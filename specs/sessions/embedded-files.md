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
   - If `filename` is specified: it is a plain string per the 2023-09 schema
     (not `@fmtstring`) — use it literally, validate it as a
     safe single path component (see [Filename path-traversal defense-in-depth](#filename-path-traversal-defense-in-depth)),
     then join with `files_directory`.
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

### Filename path-traversal defense-in-depth

Per the 2023-09 spec (§6.1.1 `<Filename>`), an embedded file's `filename` must
be a plain basename with no directory pathing. The `openjd-model` crate
enforces this at template validation time by rejecting forward-slash (`/`)
and backslash (`\`) in the filename.

As a defense-in-depth check, the sessions layer re-validates the
filename in phase 1 before joining it to the target directory. The value
must be a single safe path component:

| Rejected filename | Reason |
|-------------------|--------|
| `""` | empty |
| contains `/` or `\` | path separators |
| contains `\0` | null byte |
| exactly `.` or `..` | current/parent dir component |

Rejection surfaces as `SessionError::EmbeddedFilePath { name, filename, reason }`.
Backslash is rejected on all platforms — embedded file filenames are single
path components by spec, so `\` has no legitimate use even on POSIX.

This check is not required for correctness when the model layer is
functioning. It provides protection against:
- Future regressions or gaps in model-layer validation.
- Templates that reach the session layer via code paths that skip full model
  validation.
- Any implementation-level format-string substitution in the filename field
  (the current model stores `filename` as a `FormatString`, so a resolved
  value could differ from the raw template).

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

Environment and step scripts both use the full two-phase flow:
```
allocate_file_paths() → evaluate_let_bindings() → write_file_contents()
```

This is what makes `Env.File.*` / `Task.File.*` available to `let` bindings
while letting file `data` reference let-bound values. It is only possible
because `filename` is a plain string (2023-09 schema, not `@fmtstring`), so
path allocation never depends on `let` values.

## Wrap Environment File Path Caching

Wrap hooks (`onWrapEnvEnter`, `onWrapTaskRun`, `onWrapEnvExit`) may reference
the wrap environment's `Env.File.*` symbols. Unlike normal environment/step
scripts where each action invocation has a fresh `EmbeddedFiles` instance, wrap
hooks fire repeatedly (once per inner environment enter/exit, once per task) and
must present stable file paths across invocations.

### Design

The `Session` maintains a per-wrap-environment cache
(`wrap_env_file_records: HashMap<EnvironmentIdentifier, EmbeddedFiles>`):

- **First invocation (cache miss):** `allocate_file_paths()` allocates paths
  and writes empty files (unnamed) or validates filenames (named). The
  `EmbeddedFiles` instance is stored in the cache.
- **Subsequent invocations (cache hit):** `register_file_paths()` re-registers
  the previously allocated paths into the current action's symbol table without
  allocating new paths or creating files on disk.
- **Every invocation:** `write_file_contents()` is called AFTER
  `seed_wrapped_action_symbols()` so file `data` resolves against the post-lets
  symbol table. This means each invocation starts from the authored content,
  which may reference values that change between invocations.

### Ordering

The ordering at each wrap-hook dispatch site is:

```
register/allocate file paths → seed_wrapped_action_symbols (evaluates lets) → write file contents
```

This matches the standard two-phase flow (`allocate → lets → write`) and
ensures the wrap env's `let` bindings can reference `{{Env.File.*}}`.

### Eviction

The cache entry is removed when the wrap environment itself is exited
(`exit_environment`). The on-disk files are NOT deleted — they reside in the
session's files directory and are cleaned up with it at session end.

### `register_file_paths`

```rust
pub(crate) fn register_file_paths(&self, symtab: &mut SymbolTable) -> Result<(), SessionError>;
```

Iterates the previously allocated `FileRecord`s and sets each record's symbol
to its filename path in the symbol table. Does not allocate paths, create files,
or mutate `self.records`. Logs a "Reusing embedded file paths" message.
