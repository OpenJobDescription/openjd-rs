# Format-Aware Path Parsing

## Overview

`functions/path_parse.rs` provides path manipulation routines that honor a caller-
supplied `PathFormat` (Posix, Windows, or Uri) rather than the host OS conventions
that `std::path::Path` assumes. This module is the underlying engine for every PATH
property access (`path.name`, `path.parent`, ...) and every path method
(`with_name`, `as_posix`, ...) in the expression language.

## Why not `std::path`

`std::path::Path` on Linux uses POSIX rules; on Windows it uses Windows rules. That
is correct for host-oriented code but wrong for OpenJD: a template authored on
Windows and evaluated on a Linux worker must still interpret `C:\renders\frame.exr`
as a Windows path. Using `std::path` would silently treat the backslashes as
literal filename characters on Linux, corrupting `path.name`, `path.parent`, and
every other path operation.

The Python reference implementation solves this with
`pathlib.PurePosixPath` / `PureWindowsPath`. Rust doesn't have an equivalent
"pure-with-explicit-flavor" abstraction in std, so `path_parse.rs` implements the
same logic directly on string slices.

## Separator Handling

```rust
fn is_sep(c: char, fmt: PathFormat) -> bool;
pub fn sep(fmt: PathFormat) -> char;
```

| Format | Separators accepted | Canonical separator |
|---|---|---|
| Posix | `/` | `/` |
| Uri | `/` | `/` (opaque — see path-mapping.md) |
| Windows | `\` and `/` | `\` |

Windows accepts both because Windows APIs do, and templates commonly mix slashes
when paths come from different authoring tools.

## Anchor Detection

```rust
fn anchor_len(path: &str, fmt: PathFormat) -> usize;
```

Returns the byte length of the path's root/anchor — the prefix that must never be
stripped during normalization. Recognized anchors:

| Format | Anchor kind | Example | Anchor |
|---|---|---|---|
| Posix | Absolute | `/mnt/foo` | `/` |
| Posix | Special `//` | `//authority/p` | `//` (POSIX reserves this) |
| Windows | Drive-letter absolute | `C:\foo` | `C:\` |
| Windows | Drive-letter relative | `C:foo` | `C:` |
| Windows | Root-only absolute | `\foo` | `\` |
| Windows | UNC | `\\server\share\foo` | `\\server\share\` |

UNC detection is the main complexity: the anchor extends through the share name and
its trailing separator, so `parts()` treats the entire `\\server\share\` as a single
indivisible root component.

## Public API

```rust
pub fn sep(fmt: PathFormat) -> char;
pub fn split(path: &str, fmt: PathFormat) -> (&str, &str);   // (parent_str, name)
pub fn file_name(path: &str, fmt: PathFormat) -> &str;
pub fn parent(path: &str, fmt: PathFormat) -> String;        // returns String — may rewrite separators
pub fn file_stem(path: &str, fmt: PathFormat) -> &str;
pub fn extension(path: &str, fmt: PathFormat) -> &str;       // includes the leading dot
pub fn extension_no_dot(path: &str, fmt: PathFormat) -> &str;
pub fn parts(path: &str, fmt: PathFormat) -> Vec<String>;
pub fn with_name(path: &str, name: &str, fmt: PathFormat) -> String;
// ... and related "with_*" builders
```

## Integration with the Expression Language

Path properties and methods registered in `default_library.rs` delegate to this
module:

| Expression | Implementation |
|---|---|
| `p.name` | `path_parse::file_name` |
| `p.stem` | `path_parse::file_stem` |
| `p.suffix` | `path_parse::extension` |
| `p.parent` | `path_parse::parent` (wrapped in new PATH with same format) |
| `p.parts` | `path_parse::parts` |
| `p.with_name(x)` | `path_parse::with_name` |
| `p.with_stem(x)` | derived from `parent` + new name with extension |
| `p.with_suffix(x)` | derived from `parent` + stem + new suffix |
| `p / child` | concatenation with canonical separator |

URI paths (`PathFormat::Uri`) are handled separately by `uri_path.rs`, which does
**not** normalize. The two modules are disjoint: `path_parse` functions called with
`PathFormat::Uri` handle only trivially-structured paths (no authority, no `://`).
The expression-language dispatch checks for URI format first and routes to
`uri_path` before falling through to `path_parse`.

## Normalization

Trailing separators are stripped except when they form part of the anchor (`/`, `\`,
`C:\`, UNC shares). This matches pathlib: `"/foo/"` has name `"foo"` and parent `"/"`.

Redundant `.` and `..` segments are **not** collapsed — `path_parse` is a lexical
tool, not a filesystem one. Collapsing would require canonicalizing against a real
filesystem, which violates the expression language's "no filesystem access" design
constraint.
