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
- **Posix**: exact byte comparison; `/` is the only separator
- **Windows**: case-insensitive, treats `\` and `/` interchangeably as separators
- **URI**: scheme and authority compared case-insensitively, path compared exactly

Filesystem matching (Posix and Windows) compares the path component by
component, and mirrors Python's `PurePosixPath`/`PureWindowsPath`
`is_relative_to` on two points that component comparison alone would miss:

> **Rootedness must agree.** A relative path never matches an absolute rule and
> an absolute path never matches a relative rule, even when every component
> lines up. Python gets this from the root entry in `.parts`; component
> splitting discards it, so it is compared separately. A path is rooted if it
> starts with `/`, or — under Windows only — with `\`. Drive-anchored Windows
> paths need no special case: `C:` is kept as the first component, so a drive
> mismatch already fails component comparison.
>
> **Only the format's own separators split components.** Under Windows, `\` and
> `/` are interchangeable, so the rule `C:/Users/bob` matches the input
> `C:\Users\bob\f.txt`. Under Posix, `\` is an ordinary filename character, so
> `home\user` is a single component: the rule `/home/user` does **not** match
> the input `/home\user/f`, and the rule `/home\user` matches `/home\user/f`
> but not `/home/user/f`. This follows `PurePosixPath("/home\\user/f").parts`
> == `('/', 'home\\user', 'f')`.

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

> **URI paths in path methods.** The path properties and the
> `with_*` methods all detect URI inputs (`uri_path::is_uri`) and
> route to `uri_path::*` so the result preserves URI grammar —
> `/` separator, opaque authority, no normalization — regardless
> of the evaluator's `PathFormat`. A URI value passed through any
> of these methods stays a URI; it does not pick up the host's
> backslashes when the evaluator is in `PathFormat::Windows`. See
> the conformance test
> `2023-09/EXPR/jobs/expr2.3.2--uri-path-ops.test.yaml` for the
> end-to-end pin.

> **Empty-name guard.** `with_name`, `with_stem`, `with_suffix`,
> and `with_number` raise `ExpressionError` with the message
> `with_<op>: '<path>' has an empty name` when the input has no
> final component to operate on. That covers filesystem roots
> (`/` on POSIX, `C:\` and `\\server\share` on Windows) and bare
> URI authorities (`s3://bucket`, `s3://bucket/`). Without this
> guard the operators silently invent a filename against an
> empty stem and emit nonsensical results
> (`path("/").with_name("x")` → `"//x"`,
> `path("s3://bucket").with_suffix(".png")` → `"s3://bucket/.png"`).
> Matches Python pathlib's behaviour:
> `PurePosixPath('/').with_name('x')` raises
> `ValueError: PurePosixPath('/') has an empty name`.

> **Replacement-argument validation.** `with_name` and
> `with_stem` raise with `Invalid name '<arg>'` if the supplied
> name is empty, equal to `.`, or contains a path separator
> (`/` always; `\` on Windows). `..` is *accepted* (matches
> pathlib — it's a filename that happens to look like the
> parent-dir indicator). `with_suffix` raises with
> `Invalid suffix '<arg>'` unless the suffix is empty (which
> strips the existing suffix) or starts with `.` and is not
> just `.` and contains no separator. Multi-dot suffixes like
> `.tar.gz` are accepted. `with_stem('')` raises one of two
> messages depending on whether the path has a suffix:
> `Invalid name ''` if there's no suffix to keep, or
> `'<path>' has a non-empty suffix` if there is — both align
> with pathlib's diagnostics.

> **Relative paths stay relative.** `with_name`, `with_stem`,
> `with_suffix`, and `with_number` preserve the input's
> absoluteness. A single-component relative path
> (`path("foo").with_name("x")`) produces `"x"`, not `"/x"`,
> matching pathlib. Filesystem anchors (`/`, `C:\`, UNC
> `\\server\share\`) are preserved without doubling the
> separator (`path("/foo").with_name("x")` → `"/x"`,
> `path("C:\\foo").with_name("x")` → `"C:\\x"`).

> **Filesystem path normalization on construction.** The
> `path()` constructor canonicalizes filesystem inputs the
> same way Python pathlib does on construction:
>
> - leading `./` is stripped (`path("./foo")` → `"foo"`)
> - interior `/./` segments are stripped (`path("a/./b")` → `"a/b"`)
> - trailing separators are stripped (`path("a/b/")` → `"a/b"`)
> - runs of separators are collapsed (`path("a//b")` → `"a/b"`)
> - 3+ leading slashes collapse to 1 (`path("///foo")` → `"/foo"`)
> - exactly 2 leading POSIX slashes are preserved (`path("//foo")` → `"//foo"`)
>   per IEEE Std 1003.1-2017 §3.271 (POSIX double-slash root)
> - on Windows, all `/` become `\` (`path("foo/bar")` → `"foo\\bar"`)
> - UNC anchors always end with `\` (`path("\\\\srv\\share")` → `"\\\\srv\\share\\"`)
> - empty input becomes `"."` (`path("")` → `"."`)
> - `..` segments are NOT resolved (pathlib doesn't simplify
>   `..` because it can't safely do so without filesystem access
>   to detect symbolic links)
>
> URI inputs are explicitly NOT normalized — see the URI-paths
> note above. The OpenJD specification (wiki §1.2.1) preserves
> URI path components verbatim because schemes like S3 treat
> `a//b` and `a/b` as distinct keys.

> **Drive-relative path disambiguation on Windows.** When
> `with_name` produces a name on a relative-path parent and the
> name itself begins with `<letter>:`
> (e.g., `path("foo").with_name("x:y")`), pathlib prepends `.\`
> to disambiguate from a drive-relative path: the result is
> `".\\x:y"`, not `"x:y"`. We follow the same convention. POSIX
> has no analogous ambiguity.

### Path Operators

| Operator | Signature | Description |
|----------|-----------|-------------|
| `/` | `(path, string) -> path` | Join path components |
| `/` | `(path, path) -> path` | Join paths |
| `+` | `(path, string) -> path` | Append to last component |

On Windows filesystem paths, a root-relative right operand (for example,
`\renders`) replaces the left path below its root. A drive or UNC root from the
left operand is retained; if the left operand has no drive or UNC root, the right
operand replaces it entirely. The same absolute-right rule applies to URI left
operands, so `/renders` replaces the URI entirely under POSIX and URI formats.
Under Windows, a single leading `/` or `\` is root-relative rather than absolute;
for a URI left operand it replaces only the path below the authority. For example,
`path("s3://bucket/prefix") / "/renders"` produces `s3://bucket/renders` under
Windows and `/renders` under POSIX. A Windows drive-relative right operand replaces
a URI left operand entirely, so `path("s3://bucket/prefix") / "C:render.exr"`
produces `C:render.exr`.

### apply_path_mapping

`apply_path_mapping(path_string)` is a host-context-only function that applies
the rules captured in its implementing closure to a string, returning a path
value. It is registered on a [`FunctionLibrary`](../../crates/openjd-expr/src/function_library.rs)
obtained from [`FunctionLibrary::for_profile`](../../crates/openjd-expr/src/default_library.rs)
with an [`ExprProfile`](../../crates/openjd-expr/src/profile.rs) whose
`host_context` is `HostContext::WithRules(Arc::new(rules))` — see
[function-library.md § Host Context](function-library.md#host-context).
During template validation, when rules aren't available yet, callers build
the profile with `HostContext::Unresolved` instead, which registers a stub
that returns `Unresolved(path)` for type checking.

## Divergence from Python

The Rust implementation uses the same `PathMappingRule` structure and application logic
as the Python version. The Python version uses `PurePosixPath` / `PureWindowsPath` for
path manipulation; the Rust version uses string operations directly, which avoids the
overhead of constructing path objects for simple prefix matching.

URI path handling is functionally identical between implementations.
