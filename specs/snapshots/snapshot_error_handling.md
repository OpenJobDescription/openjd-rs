# Error Handling

[README](README.md) · Error Handling

**Location:** `error.rs`

## SnapshotError Enum

All fallible operations in the crate return `crate::Result<T>`, which is `std::result::Result<T, SnapshotError>`.

The enum is `#[non_exhaustive]`, so new variants can be added without a breaking change.

```rust
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SnapshotError {
    Io(#[from] std::io::Error),
    Validation(String),
    FileNotFound(String),
    Cancelled,
    Cache(String),
    S3(String),
    Task(String),
}
```

### Variants

| Variant | Display Format | When Used |
|---------|---------------|-----------|
| `Io(std::io::Error)` | `IO error: {inner}` | File/directory I/O failures, S3 data cache operations that surface as `std::io::Error` |
| `Validation(String)` | `Manifest validation error: {msg}` | Invalid manifest content: bad paths, missing fields, constraint violations, codec errors |
| `FileNotFound(String)` | `File not found: {path}` | A required file or directory does not exist (COLLECT `filenames` parameter) |
| `Cancelled` | `Operation cancelled` | Progress callback returned `false`, or cancellation flag was set |
| `Cache(String)` | `Cache error: {msg}` | SQLite errors from `HashCache` or `S3CheckCache` |
| `S3(String)` | `S3 error: {msg}` | AWS S3 or STS API call failures |
| `Task(String)` | `Task error: {msg}` | Tokio runtime creation, `spawn` failures, or `JoinError` from background tasks |

## Error Conversion Strategy

### Automatic conversion (`#[from]`)

Only `std::io::Error` has a `From` impl via `#[from]`. This allows `?` to convert I/O errors directly.

### Manual conversion (`.map_err()`)

All other error sources are converted to string representations via `.map_err(|e| SnapshotError::Variant(e.to_string()))`. This is a deliberate design choice — it decouples the public error API from internal dependencies:

| Source | Target Variant | Conversion |
|--------|---------------|------------|
| `rusqlite::Error` | `Cache(String)` | `.map_err(\|e\| SnapshotError::Cache(e.to_string()))` |
| AWS SDK S3/STS errors | `S3(String)` | `.map_err(\|e\| SnapshotError::S3(format!(...)))` |
| `tokio::task::JoinError` | `Task(String)` | `.map_err(\|e\| SnapshotError::Task(e.to_string()))` |
| `tokio::runtime::Builder` errors | `Task(String)` | `.map_err(\|e\| SnapshotError::Task(e.to_string()))` |
| `walkdir::Error` | `Io(std::io::Error)` | `.map_err(\|e\| SnapshotError::Io(std::io::Error::other(e.to_string())))` |
| S3 data cache I/O operations | `Io(std::io::Error)` | `.map_err(SnapshotError::Io)` or `.map_err(crate::SnapshotError::Io)` |

**Tradeoff:** Converting to `String` breaks the `source()` chain, making it harder to programmatically inspect the underlying error. However, it avoids leaking `rusqlite`, `aws-sdk-s3`, and `tokio` types into the public API, which would make those crates part of the semver contract.

### S3 errors as `std::io::Error`

The `AsyncDataCache` trait returns `std::io::Result` from its methods. The `S3DataCache` implementation wraps S3 SDK errors as `std::io::Error::other(format!(...))`. These are then converted to `SnapshotError::Io` by callers. This means some S3 failures surface as `Io` rather than `S3` — the `S3` variant is reserved for errors that occur outside the `AsyncDataCache` trait (e.g., STS `GetCallerIdentity`).

## Error Message Conventions

### Validation errors include context

Validation error messages include the offending path or value:

```
"expected absolute path, got: relative/path.txt"
"duplicate path: /tmp/foo.txt"
"file already has hashes set, cannot re-hash: /tmp/a.txt"
"file 'big.bin' with size 1024 should have 4 chunks (chunk_size=256), got 2"
```

### I/O errors include the path

When wrapping I/O errors from file operations, the path is prepended:

```rust
std::fs::File::open(&path).map_err(|e| {
    SnapshotError::Io(std::io::Error::new(e.kind(), format!("{path}: {e}")))
})?;
```

This produces messages like: `IO error: /tmp/missing.txt: No such file or directory`

### S3 errors include the key

S3 error messages include the operation and key:

```
"S3 GetObject range failed for Data/abc123.xxh128: ..."
"S3 CopyObject failed: ..."
"STS GetCallerIdentity failed: ..."
```

## Cancellation

Operations that support progress callbacks check for cancellation in two ways:

1. **Progress callback returns `false`** — the callback is invoked with current statistics; returning `false` signals cancellation.
2. **`AtomicBool` flag** — set when cancellation is detected, checked by worker threads/tasks before starting new work items.

When cancelled, the operation returns `SnapshotError::Cancelled`. In-flight work items may complete, but no new work items are started.

## Usage by Operation

| Operation | Typical Error Variants |
|-----------|----------------------|
| COLLECT | `FileNotFound`, `Io`, `Validation`, `Task` |
| HASH | `Validation`, `Io`, `Task`, `Cancelled`, `Cache` |
| HASH_UPLOAD | `Validation`, `Io`, `Task`, `Cancelled`, `Cache`, `S3` |
| DOWNLOAD | `Io`, `Task`, `Cancelled`, `Cache` |
| DIFF | `Validation` |
| COMPOSE | `Validation` |
| FILTER | (infallible) |
| SUBTREE | `Validation` |
| PARTITION | `Validation` |
| JOIN | `Validation` |
| CACHE_SYNC | `Io`, `Task`, `Cancelled` |
| Codec (encode/decode) | `Validation` |
| Manifest validate | `Validation` |

## Test Coverage

Error message formats are pinned in `tests/test_error_messages.rs`. Each variant has a test asserting the exact `Display` output, catching regressions in error message quality.
