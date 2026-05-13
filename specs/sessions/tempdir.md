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

1. Generate a unique directory name: `{prefix}{random_hex}` (16 hex chars)
2. Create the directory via `std::fs::create_dir()`
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

## Integration with Session

`Session::with_config()` creates two `TempDir` instances:

1. **Working directory**: `TempDir::new(root, "session-", user)` — the session's scratch
   space, referenced as `Session.WorkingDirectory` in the symbol table
2. **Files directory**: `TempDir::new(working_dir, "files-", user)` — subdirectory for
   embedded files, referenced by `EmbeddedFiles`

Both are stored in the `Session` struct and cleaned up by `Session::cleanup()`.
