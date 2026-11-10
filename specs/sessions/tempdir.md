# Secure Temp Directory

## Overview

`TempDir` in `tempdir.rs` provides secure temporary directory creation for session
working directories and files directories. It handles platform-specific paths,
permissions, cross-user ownership, and sticky bit validation.

## openjd_temp_dir

```rust
pub fn openjd_temp_dir(base_dir: Option<&Path>) -> Result<PathBuf, SessionError>
```

Returns the OpenJD temp directory, creating it (and any missing parents) if
needed. `base_dir` is the OpenJD directory itself.

- `None` → derive from the environment:
  - POSIX: `<std::env::temp_dir()>/OpenJD` (typically `/tmp/OpenJD`).
  - Windows: `%PROGRAMDATA%\Amazon\OpenJD` (with a warning and fallback to
    `C:\ProgramData\Amazon\OpenJD` if `PROGRAMDATA` is unset).
- `Some(p)` → use `p` directly.

Production callers pass `None`. Tests pass `Some(...)` to avoid mutating
process-global environment variables, which races with parallel tests that
read them.

### Why a custom temp directory

The system temp directory (`/tmp`) is world-writable and shared by all users. Creating
session directories directly in `/tmp` risks:
- Name collisions with other applications
- Confusion about which directories belong to OpenJD
- Missing the OpenJD-specific permission model

The `/tmp/OpenJD` subdirectory provides a namespace and allows setting appropriate
permissions on the parent.

## TempDir

```rust
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub fn new(
        dir: &Path,
        prefix: &str,
        user: Option<&PosixSessionUser>,
    ) -> Result<Self, SessionError>;

    pub fn path(&self) -> &Path;
    pub fn cleanup(self) -> Result<(), SessionError>;
}
```

### Construction

`TempDir::new(dir, prefix, user)`:

1. Generate a directory name: `{prefix}{random_hex}`, where `random_hex` is a
   v4 UUID in simple form (32 lower-case hex characters)
2. Create the directory via `std::fs::create_dir()`. An existing directory of
   that name is an error; there is no retry
3. Set permissions:
   - Same-user: 0o700 (owner rwx only)
   - Cross-user: 0o770 (owner + group rwx) after `chown` to set group
4. If `user` is provided and not the process user:
   - `chown(path, None, Some(group_gid))` — set group ownership
   - This allows the cross-user subprocess to read/write in the directory

### Drop implementation — best-effort cleanup

The Python library's `TempDir` has explicit `cleanup()` and no `__del__`. The Rust crate
goes further: `cleanup()` is the primary API, but `TempDir` also implements `Drop` as a
safety net that performs a best-effort `remove_dir_all` if `cleanup()` was never called.
Errors from the `Drop` path are silently ignored (no async, no error propagation).

Callers should still prefer explicit `cleanup()` because:

1. `retain_working_dir` may be set — the `Session` checks this before calling `cleanup()`
2. Cross-user cleanup requires `sudo rm` before the normal removal — `Drop` can't do this
3. The `Session` manages cleanup timing — it needs to exit environments before deleting
   the directory

The `Session` struct's own `Drop` impl logs a warning if `Session::cleanup()` wasn't
called, and also performs a best-effort directory removal, providing a safety net for
debugging without silently leaking temp directories.

## Sticky Bit Validation

```rust
pub fn validate_sticky_bit(root_dir: &Path)
```

On POSIX, checks parent directories for world-writable directories missing the sticky
bit. A world-writable directory without the sticky bit allows any user to rename or
delete files created by other users — a security risk for session working directories.

If found, logs a warning via `log::warn!`. Does not fail — the session continues, but
the operator is alerted to fix the directory permissions.

### Why warn instead of fail

The sticky bit check is a defense-in-depth measure. `/tmp` on most Linux distributions
has the sticky bit set, so this warning rarely fires. Failing would break sessions on
misconfigured systems where the operator may not have control over `/tmp` permissions.

## Session working directory name

The session working directory is created by the crate-internal
`TempDir::new_session_working_dir(root, user)`, not by `TempDir::new`. Its name
is 8 random characters and nothing else:

- **Alphabet.** `abcdefghijklmnopqrstuvwxyz0123456789_`, drawn uniformly from
  the OS CSPRNG (`getrandom`, with rejection sampling). Lower case only, so
  two names never differ only in case (which case-insensitive filesystems
  treat as the same name), and no character is reserved on Windows. An
  8-character name cannot be a Windows device name (`CON`, `COM1`, ...).
- **Collisions.** Each candidate is created with an exclusive
  `std::fs::create_dir()`. If the name already exists, a fresh name is tried,
  up to 10,000 attempts. An existing directory is never reused, and concurrent
  sessions in one root never share a path. Any other error is returned at
  once. If every attempt collides, creation fails with `SessionError::TempDir`
  naming the root and `No usable temporary directory name found after 10000
  attempts`.
- **Permissions** are applied as for `TempDir::new` (see Construction above).

### Why 8 random characters

Windows applications that a job runs, such as After Effects and Cinema 4D,
use the legacy Win32 file APIs and so are limited to `MAX_PATH` (260
characters) regardless of the `LongPathsEnabled` registry value or a
`\\?\` prefix. The working directory's path counts against that limit for
everything beneath it. A short random name keeps that cost low while staying
unique.

The name carries no part of the session ID. To trace a retained or orphaned
directory to its session, use the session log: `Session::with_config()` logs
`Initializing Open Job Description Session: <session_id>` and
`Session Working Directory: <path>`, and every `session_log!` record carries
the session ID.

### Differences from Python

This matches openjd-sessions-for-python PR #348, with two differences:

- This crate makes at most 10,000 attempts on every platform. Python's limit
  depends on the platform.
- On Windows, Python also retries when creating the directory fails with
  `PermissionError` in a writable parent, because a name pending deletion
  reports access denied. This crate retries only on `AlreadyExists`, so that
  case fails the session. It needs a random name to match one pending
  deletion out of 37^8 (about 3.5 × 10^12) names, so the simpler rule was
  kept.

## Integration with Session

`Session::with_config()` creates two directories:

1. **Working directory**: `TempDir::new_session_working_dir(root, user)` — the
   session's scratch space, named as described above, referenced as
   `Session.WorkingDirectory` in the symbol table
2. **Files directory**: `TempDir::new(working_dir, "embedded_files", user)` —
   subdirectory for embedded files, referenced by `EmbeddedFiles`, named
   `embedded_files` followed by 32 hex characters. Python uses 8 random
   characters after `embedded_files`; that difference is not yet resolved.

Both are stored in the `Session` struct and cleaned up by `Session::cleanup()`.

### Cleanup retries on Windows

On Windows, `TempDir::cleanup()` blocks for at least about 31 s once something is held
open — the five retry waits — plus the time each pass spends walking the tree, which
grows with the number of files. A process that has exited
or been terminated can still hold handles in the directory for a while, and Windows
will not delete a file while a handle to it is open without `FILE_SHARE_DELETE`. So
cleanup runs a removal pass, and retries a pass that leaves anything behind after 1,
2, 4, 8 and 16 s. `NotFound` on a retry counts as success, because the pass that
failed may still have removed the directory. `NotFound` on the first pass is still
an error. Other platforms do not retry: a failure there is a permission problem that
waiting will not fix. They use `std::fs::remove_dir_all`, which stops at the first
error.

#### The Windows removal pass

Each pass (`win32_remove::remove_tree`) deletes everything it can and records each
entry it could not, as Python's `shutil.rmtree` does with an error handler. One
locked file therefore leaves only that file and the directories above it.
`std::fs::remove_dir_all` is not used here because it stops at the first error — it
deletes what it reached before then, but abandons the rest of the tree and does not
say which entry failed.

The session directory is writable by the job user, and the processes cleanup waits
for belong to the job, so the walk must not be tricked into following a junction or
symlink planted mid-walk:

- A child is classified with `DirEntry::file_type`, which does not follow a link,
  so a symlink or junction is removed as a link and its target is never traversed.
  Any directory the walk opens uses `FILE_FLAG_OPEN_REPARSE_POINT` and is refused
  if it turns out to be a reparse point: "it is a link or reparse point, so it was
  not followed", recorded as a failure.
- Each directory on the path being walked is held open without
  `FILE_SHARE_DELETE` while its entries are removed, so it cannot be renamed or
  replaced. This matches the time-of-check/time-of-use safety of
  `std::fs::remove_dir_all` (CVE-2022-21658). One case it does not cover: a
  directory another process holds open with `DELETE` access and full sharing
  cannot be pinned, so the walk records it as a failure and does not descend into
  it (see the parity table). The walk is iterative, so depth does not bound it.

A read-only file or directory has the attribute cleared, through a handle, before it
is deleted, as `std::fs::remove_dir_all` does.

#### The warning

Each failed Windows pass is logged as a warning with content
`FILE_PATH | PROCESS_CONTROL | EXCEPTION_INFO`. It names the directory and the first
error of the pass, then how many files could not be deleted and up to three of
them, then the processes holding any of the first 256 of those open, found with the
Restart Manager (`RmGetList`). It ends with the next delay, or "Giving up." after
the last pass:

```
Could not delete temporary directory C:\ProgramData\Amazon\OpenJD\c46tnxjh: The process
cannot access the file because it is being used by another process. 1 file(s) could
not be deleted, including a_sub\locked.bin. Files are still locked open by:
powershell.exe (pid 3604). Retrying in 4 s.
```

When no file failed (for example, a directory is some process's current directory),
the line names only the directory and the error. A failed holder lookup replaces
the process list with "The processes holding them open could not be determined:
<reason>.", and an empty one with "No running process was found holding them open."

The holder lookup registers only files, not directories: registering a directory
makes the whole `RmGetList` call fail, so a directory would cost the lookup for the
files beside it.

#### Python parity

The retry schedule, the files reported, and the warning text match Python's
`TempDir.cleanup` (openjd-sessions-for-python #376), including the error text
without Rust's "(os error N)" suffix. The differences are intentional:

| | Rust | Python |
|---|---|---|
| Read-only files and directories on Windows | Attribute cleared, then deleted | `rmtree` fails on them, so they are reported as not deleted |
| Count of undeletable files in the warning | The true count, even above 256 | Capped at the 256 it inspects for holders |
| A failed holder lookup | `… could not be determined: <reason>.` | Prefixes the exception type: `… could not be determined: OSError: <reason>.` |
| A directory another process holds with `DELETE` access | Not descended into (cannot be pinned); recorded as a failure | `rmtree` deletes its unlocked contents |
| A directory swapped for a junction mid-walk | Not followed (see above) | `rmtree` on Windows does not defend against this (`shutil.rmtree.avoids_symlink_attacks` is `False`) |
| After the last pass | `SessionError::TempDir` with the first error, logged by `Session::cleanup()` as "Files within temporary directory <path> could not be deleted: <reason>" | `RuntimeError` listing every path that failed, with its error |

`Session::cleanup()` removes only the working directory, which contains the embedded
files directory, so a lock inside the latter is waited out once. It logs each directory
it could not remove at error level, with the path and the reason, and continues.
