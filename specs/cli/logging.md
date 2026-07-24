# Session Logger

## Purpose

The CLI implements a custom `log::Log` to display real-time subprocess output during
`run` command execution. The logger filters log records by structured metadata to show
only subprocess command output (not internal debug/info messages), and formats each line
with a timestamp.

## Architecture

```
openjd-sessions subprocess loop
  │
  ├── Reads stdout line from child process
  ├── Passes through ActionFilter (openjd_* directive parsing)
  └── log::info!(openjd_log_content = LogContent::COMMAND_OUTPUT.bits(); "{line}")
      │
      └── SessionLogger::log(&record)
          ├── Extract openjd_log_content from record's key-value pairs
          ├── Check if COMMAND_OUTPUT bit (0x08) is set
          └── If set: println!("{timestamp}\t{message}")
```

## LogContent Filtering

The sessions crate uses `log`'s key-value feature to attach structured metadata to log
records. The `openjd_log_content` key carries a `LogContent` bitflag value that classifies
the log line's content type.

The CLI's `SessionLogger` only displays lines with the `COMMAND_OUTPUT` bit set (bit 3,
value 8). This filters out internal session lifecycle messages, action status updates, and
other metadata that would clutter the user's terminal output.

### Visitor Pattern

The `log::Log::log()` method receives a `&Record` whose key-value pairs must be extracted
via the visitor pattern:

```rust
struct Visitor { bits: Option<u64> }

impl<'kvs> log::kv::VisitSource<'kvs> for Visitor {
    fn visit_pair(
        &mut self,
        key: log::kv::Key<'kvs>,
        value: log::kv::Value<'kvs>,
    ) -> Result<(), log::kv::Error> {
        if key.as_str() == "openjd_log_content" {
            self.bits = value.to_u64();
        }
        Ok(())
    }
}
```

This is verbose but necessary — `log`'s kv API doesn't provide direct key lookup. The
visitor iterates all key-value pairs and captures the one we care about.

## Timestamp Formatting

Three timestamp formats are supported, controlled by the `--timestamp-format` argument:

### Relative (default)

Elapsed time since session start in `H:MM:SS.mmm` format:

```
0:00:00.000    Session start
0:00:01.234    Hello from task
0:01:30.567    Task complete
```

Uses `SESSION_START` global (`OnceLock<Instant>`) set at the beginning of `run::execute()`.
If the global hasn't been set (shouldn't happen in normal flow), falls back to `Instant::now()`.

### Local

Local timezone timestamp in ISO 8601 format:

```
2024-03-15T14:30:45.123    Session start
2024-03-15T14:30:46.357    Hello from task
```

Uses `chrono::Local::now()`.

### UTC

UTC timestamp in ISO 8601 format with Z suffix:

```
2024-03-15T21:30:45.123Z    Session start
2024-03-15T21:30:46.357Z    Hello from task
```

Uses `chrono::Utc::now()`.

## Global State

Two `OnceLock` globals store logger configuration:

```rust
static SESSION_START: OnceLock<Instant> = OnceLock::new();
static TIMESTAMP_FORMAT: OnceLock<String> = OnceLock::new();
```

These are set once by `run::execute()` and read by `format_log_timestamp()` on every log
call. `OnceLock` ensures thread-safe initialization without runtime overhead after the
first write.

`RunContext::timestamp()` delegates to the same `format_log_timestamp()` function for
directly printed status messages (for example, "Session start" and "Running step 'X'").
Logger output and command status output therefore use identical timestamp formatting.

## Log Level Control

The logger is initialized with `LevelFilter::Info` in `main()`. When `--verbose` is
passed, `run::execute()` upgrades to `LevelFilter::Debug`:

```rust
if args.verbose {
    log::set_max_level(log::LevelFilter::Debug);
}
```

This affects all log records globally, including internal sessions crate debug messages.
The `SessionLogger` still filters on `openjd_log_content`, so verbose mode primarily
affects whether debug-level subprocess output lines are displayed.

## Differences from Python CLI

| Aspect | Python | Rust |
|--------|--------|------|
| Logger type | `LocalSessionLogHandler` (logging.Handler subclass) | `SessionLogger` (log::Log impl) |
| State | Instance fields (start_time, format, entries list) | Global OnceLock statics |
| Log storage | Stores `LogEntry` objects in memory for result output | No storage; prints directly to stdout |
| Real-time output | Configurable via `print_to_stdout` flag | Always prints to stdout |
| Timestamp | `LoggingTimestampFormat` enum with `format_timestamp()` | `format_log_timestamp()` free function |

The Python CLI stores all log entries in memory and includes them in the structured result
output (`OpenJDRunResult.logs`). The Rust CLI prints logs in real-time but doesn't store
them — the structured result output includes task count and duration but not log content.
