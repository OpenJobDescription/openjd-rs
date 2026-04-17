# Path Mapping

## Overview

Path mapping transforms filesystem paths between different formats and locations. This
is essential for render farms where jobs are authored on one OS but executed on another,
or where storage mount points differ between submission and execution hosts.

Defined in `path_mapping.rs` (PathFormat, PathMappingRule) and `uri_path.rs` (URI-aware
path operations).

## PathFormat

```rust
pub enum PathFormat {
    Posix,    // Forward slashes, case-sensitive
    Windows,  // Backslashes (normalized to forward), case-insensitive matching
    Uri,      // scheme://authority/path — no normalization
}
```

`PathFormat::host()` returns the format for the current platform (`Posix` on Linux/macOS,
`Windows` on Windows).

The evaluator carries a `PathFormat` that controls how path values are normalized. PATH
values carry their own format and are validated against the evaluator's format. URI paths
bypass normalization entirely.

## PathMappingRule

```rust
pub struct PathMappingRule {
    pub source_path_format: PathFormat,
    pub source_path: String,
    pub destination_path: String,
}
```

### Application

`rule.apply(path) -> Option<String>` returns `Some(mapped)` if `path` starts with the
rule's `source_path` (using format-appropriate comparison), or `None` if no match.
When matched, the source prefix is replaced with `destination_path`. Trailing
slashes are preserved. Output separators use the host-native format — use
`apply_with_format` for explicit control.

Format-appropriate comparison means:
- **Posix**: exact byte comparison
- **Windows**: case-insensitive, normalizes backslashes to forward slashes
- **URI**: scheme and authority compared case-insensitively, path compared exactly

### apply_with_format

`rule.apply_with_format(path, output_format) -> Option<String>` is the same as
`apply` but lets the caller pick the output separator (`/` for Posix/Uri, `\` for
Windows). The source format still dictates how the prefix match is performed.

### Serde

`PathMappingRule` implements `Serialize` and `Deserialize` for JSON interchange:

```json
{
    "source_path_format": "WINDOWS",
    "source_path": "C:/projects",
    "destination_path": "/mnt/projects"
}
```

## URI Path Operations

`uri_path.rs` provides URI-aware path manipulation for paths with `scheme://authority/path`
structure. These are used when `PathFormat::Uri` is detected.

All functions live in the `uri_path` module — callers use `uri_path::name(p)`,
`uri_path::parent(p)`, etc. No `uri_`-prefixed names exist at the function level
because the module already provides that namespace.

| Function | Purpose |
|----------|---------|
| `is_uri(s)` | Check if string has a `scheme://` prefix |
| `parse(s) -> Option<UriParts>` | Parse into `{ authority, path_parts }` or `None` |
| `name(s)` | Last component (like `Path.name`) |
| `parent(s)` | Parent URI (like `Path.parent`) |
| `suffix(s)` | File extension |
| `suffixes(s)` | All extensions (e.g., `.tar.gz` → `[".tar", ".gz"]`) |
| `stem(s)` | Filename without extension |
| `parts(s)` | Split into components: `[authority, seg1, seg2, ...]` |
| `join(s, child)` | Append a child component to a URI path |
| `from_parts(parts)` | Reconstruct URI from `parts(...)` output |

### Why URI paths are opaque

URI paths are not normalized: consecutive slashes, `.`, and `..` are preserved
verbatim. The rationale is threefold:

- **Round-trip fidelity.** URIs can encode application-specific meaning in slash
  patterns (S3 prefixes, HTTP path parameters); folding `//` into `/` would silently
  rewrite a user-supplied key.
- **Authority preservation.** The `scheme://host[:port]` portion is a single opaque
  unit that must survive every operation. URI parsing splits it once up front
  (`parse`) and never touches it again.
- **Percent-encoded segments.** Segments may contain `%2F` which decodes to `/` but
  must not be treated as a separator. Rather than try to teach the path functions
  about URI encoding, we leave every byte of the path exactly as the user supplied
  it.

## Expression Language Integration

Path-related operations in the expression language:

### Path Properties (via `__property_NAME__`)

| Property | Type | Description |
|----------|------|-------------|
| `.name` | string | Last component of the path |
| `.stem` | string | Last component without extension |
| `.suffix` | string | File extension (including dot) |
| `.suffixes` | list[string] | All extensions |
| `.parent` | path | Parent directory |
| `.parts` | list[string] | All path components |

### Path Methods

| Method | Signature | Description |
|--------|-----------|-------------|
| `with_name(name)` | `(path, string) -> path` | Replace last component |
| `with_stem(stem)` | `(path, string) -> path` | Replace stem, keep extension |
| `with_suffix(suffix)` | `(path, string) -> path` | Replace extension |
| `with_number(n)` | `(path, int) -> path` / `(string, int) -> path` | Append frame number |
| `as_posix()` | `(path) -> string` | Convert to POSIX string |
| `is_absolute()` | `(path) -> bool` | Check if path is absolute |
| `is_relative_to(other)` | `(path, path) -> bool` | Check prefix relationship |
| `relative_to(other)` | `(path, path) -> path` | Compute relative path |

### Path Operators

| Operator | Signature | Description |
|----------|-----------|-------------|
| `/` | `(path, string) -> path` | Join path components |
| `/` | `(path, path) -> path` | Join paths |
| `+` | `(path, string) -> path` | Append to last component |

### apply_path_mapping

`apply_path_mapping(path_string)` is a host-context-only function that applies the
evaluator's path mapping rules to a string, returning a path value. It's unavailable
during template validation (no path mapping rules in that context) and available during
runtime evaluation on a worker host.

## Divergence from Python

The Rust implementation uses the same `PathMappingRule` structure and application logic
as the Python version. The Python version uses `PurePosixPath` / `PureWindowsPath` for
path manipulation; the Rust version uses string operations directly, which avoids the
overhead of constructing path objects for simple prefix matching.

URI path handling is functionally identical between implementations.
