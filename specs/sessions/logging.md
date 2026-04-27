# Structured Logging

## Overview

`logging.rs` provides structured logging for the sessions crate using the `log` crate's
`kv` feature. Every log record carries structured key-value metadata (`session_id` and
`openjd_log_content`) that consumers use to route and filter log output.

## LogContent

```rust
bitflags! {
    pub struct LogContent: u32 {
        const BANNER          = 0b0000_0001;
        const FILE_PATH       = 0b0000_0010;
        const FILE_CONTENTS   = 0b0000_0100;
        const COMMAND_OUTPUT  = 0b0000_1000;
        const EXCEPTION_INFO  = 0b0001_0000;
        const PROCESS_CONTROL = 0b0010_0000;
        const PARAMETER_INFO  = 0b0100_0000;
        const HOST_INFO       = 0b1000_0000;
    }
}
```

### Why bitflags

The Python library uses `enum.Flag` for `LogContent`, which supports bitwise OR for
combining categories. The `bitflags` crate provides the same semantics in Rust. A log
record can have multiple content categories (e.g., `BANNER | PROCESS_CONTROL`).

The worker agent filters log records by `LogContent` to decide routing:
- `COMMAND_OUTPUT` → CloudWatch (customer-visible)
- `PROCESS_CONTROL` → worker agent logs (operational)
- `HOST_INFO` → session initialization logs

## session_log! Macro

```rust
macro_rules! session_log {
    ($level:ident, $session_id:expr, $content:expr, $($arg:tt)*) => {
        log::$level!(
            session_id = $session_id,
            openjd_log_content = $content.bits();
            $($arg)*
        );
    };
}
```

### Why a macro instead of a function

The `log` crate's macros (`log::info!`, etc.) capture the caller's module path and line
number. Wrapping them in a function would report the logging module's location instead
of the actual call site. A macro preserves the correct source location.

The `kv` feature syntax (`key = value;`) is only available in the `log` macros, not
through a programmatic API, which further necessitates a macro wrapper.

## Banner Helpers

```rust
pub fn log_section_banner(session_id: &str, title: &str);
pub fn log_subsection_banner(session_id: &str, title: &str);
```

Emit formatted banner lines matching the Python library's output:

### Section banner format

```
<blank line>
==============================================
--------- {title}
==============================================
```

The separator line is exactly 46 `=` characters. The title line is prefixed with
`--------- ` (9 dashes + space). Each line is emitted as a separate `session_log!`
call with `LogContent::BANNER`.

### Subsection banner format

```
----------------------------------------------
{title}
----------------------------------------------
```

The separator line is exactly 46 `-` characters. The title line has no prefix.

**Stability note**: The banner format (separator length, prefix characters) is not
part of the public API contract. Consumers should not parse banner lines to extract
structured information — use the `openjd_log_content` key-value metadata instead.

These are used at session lifecycle boundaries (enter environment, run task, cleanup)
to provide visual structure in log output.

## Logging Coverage

| Module | Content Type | What's Logged |
|--------|-------------|---------------|
| `session.rs` | `HOST_INFO` | Version, platform, architecture at init |
| `session.rs` | `FILE_PATH` | Working directory, files directory paths |
| `session.rs` | `BANNER` | Section banners for enter/exit/run/cleanup |
| `subprocess.rs` | `PROCESS_CONTROL` | PID start, SIGTERM, SIGKILL, exit code, spawn failures |
| `subprocess.rs` | `COMMAND_OUTPUT` | Stdout/stderr lines from the subprocess |
| `subprocess.rs` | `BANNER` | Output header banner |
| `runner/env_script.rs` | `BANNER` | Subsection banner before action execution |
| `runner/step_script.rs` | `BANNER` | Subsection banner before action execution |
| `embedded_files.rs` | `FILE_PATH` | File write paths |
| `embedded_files.rs` | `FILE_CONTENTS` | File data content (debug level only) |

## Consumer Integration

Log consumers (e.g., the worker agent) inspect the `openjd_log_content` key-value pair
on each log record to determine routing. The value is a `u32` bitfield that can be
decoded back to `LogContent` flags:

```rust
// In the consumer's log handler:
if let Some(content) = record.key_values().get("openjd_log_content") {
    let flags = LogContent::from_bits_truncate(content.to_u64().unwrap() as u32);
    if flags.contains(LogContent::COMMAND_OUTPUT) {
        // Route to CloudWatch
    }
}
```

This structured approach avoids parsing log message text to determine content type,
which would be fragile and slow.
