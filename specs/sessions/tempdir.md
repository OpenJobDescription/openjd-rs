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

- **Alphabet.** `abcdefghijklmnopqrstuvwxyz0123456789_`, CPython's
  `tempfile` alphabet, drawn uniformly from the OS CSPRNG (`getrandom`, with
  rejection sampling). Lower case only, so two names cannot differ only in
  case (NTFS folds case), and free of every Windows-reserved character. An
  8-character name cannot be a Windows device name (`CON`, `COM1`, ...).
- **Collisions.** Each candidate is created with an exclusive
  `std::fs::create_dir()`. If the name already exists, a fresh name is drawn
  and tried, up to 10,000 times (CPython's `tempfile.TMP_MAX` fallback). An
  existing directory is never reused, and concurrent sessions in one root
  cannot be handed the same path. Any other error is returned at once. If
  every attempt collides, creation fails with `SessionError::TempDir` naming
  the root and `No usable temporary directory name found after 10000 attempts`.
- **Permissions** are applied exactly as for `TempDir::new` (see
  Construction above).

### Why 8 random characters

Windows applications that a job runs, such as After Effects and Cinema 4D,
use the legacy Win32 file APIs, which cap a path at `MAX_PATH` (260
characters) regardless of the `LongPathsEnabled` registry value or a `\\?\`
prefix. Only a shorter path helps them. Every character of the
working directory's path is charged against that budget for everything the
job puts beneath it. The name used to be the session ID followed by
`random_hex`: 72 characters for a Deadline Cloud session ID
(`session-` + 32 hex characters). Uniqueness needs only the random part.

The name carries no part of the session ID, so a retained or orphaned
directory cannot be traced to its session by its name. The session log does
that: `Session::with_config()` logs `Initializing Open Job Description
Session: <session_id>` and `Session Working Directory: <path>`, and every
`session_log!` record carries the session ID.

### Python parity

This matches openjd-sessions-for-python PR #348, which creates the working
directory with `TempDir(prefix="")` and so with `mkdtemp()`'s 8 random
characters. Two documented differences remain:

- CPython retries `os.TMP_MAX` times (238,328 on Linux); this crate retries a
  fixed 10,000 on every platform.
- On Windows, CPython's `mkdtemp()` also retries on `PermissionError` when the
  parent directory is a writable directory (a name pending deletion reports
  access denied). This crate retries only on `AlreadyExists` and returns
  every other error, so such a collision fails the session instead of being
  retried. CPython guards that retry with an `os.access(parent, W_OK)` check
  so that a genuinely unwritable parent fails at once; without an equivalent
  check, retrying on access denied would spin through all 10,000 attempts
  before reporting a real permission error. The case it covers needs a
  random 8-character name to match one pending deletion (37^8, about
  3.5 × 10^12, names), so the simpler rule was kept.

## Integration with Session

`Session::with_config()` creates two directories:

1. **Working directory**: `TempDir::new_session_working_dir(root, user)` — the
   session's scratch space, named as described above, referenced as
   `Session.WorkingDirectory` in the symbol table
2. **Files directory**: `TempDir::new(working_dir, "embedded_files", user)` —
   subdirectory for embedded files, referenced by `EmbeddedFiles`, named
   `embedded_files` followed by 32 hex characters. Python names it
   `embedded_files` followed by `mkdtemp()`'s 8 characters; that difference
   is not yet resolved.

Both are stored in the `Session` struct and cleaned up by `Session::cleanup()`.
