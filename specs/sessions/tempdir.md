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
    cleaned_up: bool,
}

impl TempDir {
    pub fn new(
        dir: Option<&Path>,
        prefix: Option<&str>,
        user: Option<&dyn SessionUser>,
    ) -> Result<Self, SessionError>;

    pub fn path(&self) -> &Path;
    pub fn cleanup(&mut self) -> Result<(), SessionError>;
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
5. If step 3 or 4 fails, remove the directory created in step 2 and return the
   corresponding error: `SessionError::PathPermissions` for a `chown` or DACL
   failure, `SessionError::TempDir` for a `set_permissions` failure

Step 5 matters because the failure happens after the directory exists but before
the `TempDir` value does, so the `Drop` safety net below cannot clean it up. A
`chown` to a group the process user does not belong to is the ordinary way to
reach it. The removal is best-effort on the same terms as the `Drop` path below:
its result is ignored, so a directory can survive if the removal itself fails.

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
#[cfg(unix)]
pub fn find_missing_sticky_bit(root_dir: &Path) -> Option<PathBuf>;
```

On POSIX, checks parent directories for world-writable directories missing the sticky
bit, returning the first offending path or `None` if all parents are safe. A
world-writable directory without the sticky bit allows any user to rename or delete
files created by other users — a security risk for session working directories.

The function only reports; what happens next is the caller's policy decision.
`Session::with_config()` applies `SessionConfig::sticky_bit_policy`
(`StickyBitPolicy`, default `Strict`):

| Policy | Behavior on a hit |
|---|---|
| `Strict` (default) | Returns `Err(SessionError::PathPermissions)`, naming the path and suggesting `chmod +t` or `StickyBitPolicy::Warn` |
| `Warn` | Logs via `log::warn!` to target `openjd.sessions` and continues |
| `Disabled` | Skips the check entirely |

### Why the default fails closed

Refusing the session is the secure choice: a caller that has not thought about the
risk gets the safe behavior. `/tmp` on most Linux distributions has the sticky bit
set, so `Strict` rarely fires in practice. An operator who cannot control the
offending directory's permissions can opt into `Warn` or `Disabled` deliberately,
which makes the decision explicit rather than silent.

## Integration with Session

`Session::with_config()` creates two `TempDir` instances:

1. **Working directory**: `TempDir::new(Some(&root_dir), Some(&config.session_id),
   config.user.as_deref())` — the session's scratch space, referenced as
   `Session.WorkingDirectory` in the symbol table. The prefix is the session id, so
   the directory name identifies the session that owns it.
2. **Files directory**: `TempDir::new(Some(working_dir.path()), Some("embedded_files"),
   config.user.as_deref())` — subdirectory for embedded files, referenced by
   `EmbeddedFiles`

Both are stored in the `Session` struct and cleaned up by `Session::cleanup()`.
