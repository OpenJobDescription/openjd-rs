# Embedded Files

## Overview

`EmbeddedFiles` in `embedded_files.rs` implements the two-phase file materialization
required by the OpenJD spec. Embedded files are TEXT files written to the session's files
directory before each action runs, with format strings in their `data` field resolved
against the current symbol table.

## Two-Phase API

```rust
pub struct EmbeddedFiles {
    files: Vec<EmbeddedFileInfo>,
    scope: EmbeddedFilesScope,
    user: Option<Arc<dyn SessionUser>>,
}

impl EmbeddedFiles {
    pub fn new(scope: EmbeddedFilesScope, session_files_directory: PathBuf, session_id: &str) -> Self;
    pub fn with_user(self, user: Option<Arc<dyn SessionUser>>) -> Self;

    pub fn allocate_file_paths(
        &mut self,
        files: &[EmbeddedFile],
        files_directory: &Path,
        symtab: &mut SymbolTable,
    ) -> Result<(), SessionError>;

    pub fn write_file_contents(
        &self,
        symtab: &SymbolTable,
        library: &FunctionLibrary,
        path_mapping_rules: &[PathMappingRule],
    ) -> Result<(), SessionError>;
}
```

### Why two phases

The two-phase design exists because of a circular dependency between let bindings and
embedded files (see [runners.md](runners.md) for the full explanation):

- Let bindings may reference `Env.File.<name>` / `Task.File.<name>` paths
- Embedded file `data` may reference let-bound values
- File paths must be known before let bindings are evaluated
- File contents must be written after let bindings are evaluated

Phase 1 (`allocate_file_paths`) resolves the path question. Phase 2
(`write_file_contents`) resolves the content question.

## EmbeddedFilesScope

```rust
pub enum EmbeddedFilesScope {
    Env,   // Env.File.<name>
    Step,  // Task.File.<name>
}
```

Determines the symbol table prefix for file path registration. Environment-scoped files
use `Env.File.*`, step-scoped files use `Task.File.*`.

## Phase 1: allocate_file_paths

For each embedded file:

1. Determine the file path:
   - If `filename` is specified: `files_directory / filename`
   - Otherwise: `files_directory / {random_hex}` (hash-based name for uniqueness)
2. Create an empty file with 0o600 permissions (POSIX) to reserve the path
3. Register the path in the symbol table as `ExprValue::Path`:
   - `Env.File.<name>` for environment scope
   - `Task.File.<name>` for step scope

### Why create empty files during allocation

Creating the file during allocation (rather than waiting for phase 2) ensures:
- The path is valid and writable before let bindings reference it
- No race condition if multiple embedded files target the same directory
- File permissions are set early for cross-user scenarios

## Phase 2: write_file_contents

For each allocated file:

1. Resolve the `data` format string against the (now let-binding-enriched) symbol table
2. Apply end-of-line conversion:
   - `AUTO` / `None`: platform-native (`\n` on POSIX, `\r\n` on Windows)
   - `LF`: force `\n`
   - `CRLF`: force `\r\n`
3. Write the resolved content to the file
4. If `runnable` is true, set execute permission (0o700 on POSIX)
5. If cross-user, set group ownership and permissions via `chown_for_user()`

## Cross-User File Permissions

When a `PosixSessionUser` is set via `with_user()`:

`chown_for_user(path, user)`:
1. Look up the user's group GID
2. `chown(path, -1, gid)` — set group ownership without changing owner
3. Set permissions to allow group read/write (and execute if runnable)

This ensures the cross-user subprocess can read embedded files written by the session
process. The Python library does the same via `os.chown` and `os.chmod`.

## End-of-Line Conversion

The `FEATURE_BUNDLE_1` extension adds the `endOfLine` field to embedded files:

| Value | Behavior |
|-------|----------|
| `AUTO` (default) | Platform-native line endings |
| `LF` | Force Unix line endings |
| `CRLF` | Force Windows line endings |

The conversion is applied after format string resolution, ensuring that expressions
that produce multi-line strings get consistent line endings.

## Integration with Runners

Environment scripts use the full two-phase flow:
```
allocate_file_paths() → evaluate_let_bindings() → write_file_contents()
```

Step scripts use a simplified flow (let bindings evaluated first):
```
evaluate_let_bindings() → allocate_file_paths() + write_file_contents()
```

See [runners.md](runners.md) for why the ordering differs.
