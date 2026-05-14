# Action Filter

## Overview

`ActionFilter` in `action_filter.rs` parses `openjd_*` directives from subprocess stdout
lines. It is the Rust equivalent of the Python `ActionMonitoringFilter`, but implemented
as a standalone struct rather than a `logging.Filter` subclass.

## Directive Protocol

The OpenJD specification defines stdout messages that actions emit to communicate with
the runtime. Each directive is a single line matching `^openjd_<kind>: <payload>$`:

| Directive | Payload | Effect |
|-----------|---------|--------|
| `openjd_progress` | Float 0–100 | Update action progress percentage |
| `openjd_status` | String | Update human-readable status message |
| `openjd_fail` | String | Set failure reason, trigger cancelation |
| `openjd_env` | `NAME=VALUE` | Set environment variable for subsequent actions |
| `openjd_unset_env` | `NAME` | Unset environment variable |
| `openjd_redacted_env` | `NAME=VALUE` | Set env var + redact value in logs |
| `openjd_session_runtime_loglevel` | `DEBUG\|INFO\|WARNING\|ERROR` | Change log level |

## ActionFilter Interface

```rust
pub struct ActionFilter {
    session_id: String,
    echo_openjd_directives: bool,
    redactions_enabled: bool,
    redacted_values: HashSet<String>,
    redacted_lines: HashSet<String>,
    log_level: u32,
}

impl ActionFilter {
    pub fn new(session_id: &str, echo_openjd_directives: bool, redactions_enabled: bool) -> Self;

    pub fn filter_message(&mut self, line: &str, session_id: &str)
        -> (Vec<FilterCallback>, bool, String);

    pub fn min_log_level(&self) -> u32;

    pub fn add_redacted_values(&mut self, values: &[String]);
}
```

### FilterCallback

Rather than returning `ActionMessage` values directly, the filter returns
`FilterCallback` structs that decouple the filter from the session's message types:

```rust
pub struct FilterCallback {
    pub kind: ActionMessageKind,
    pub value: ActionMessageValue,
    pub cancel: bool,
}
```

The `cancel` flag indicates that the action should be canceled and marked as failed
(e.g., due to a malformed directive). The caller in `subprocess.rs` maps these to
`ActionMessage` variants and sends them through the mpsc channel.

### Why `session_id` is a parameter

The filter stores a `session_id` at construction and compares it against the
`session_id` parameter on each call. Lines from a different session are passed through
unmodified. This supports shared log streams where multiple sessions interleave output.

### Why `filter_message` returns a tuple

The return value `(Vec<FilterCallback>, bool, String)` contains:

1. `Vec<FilterCallback>` — Zero or more parsed callbacks (usually 0 or 1)
2. `bool` — Whether the line should be passed through to logging (`true` when
   `echo_openjd_directives` is true for recognised directives; always `true`
   for non-directive lines)
3. `String` — The (possibly redacted) line for logging

This three-part return avoids the Python pattern of mutating shared state from a logging
filter callback. The caller decides what to do with each part.

`echo_openjd_directives` is the same flag that appears on
[`SessionConfig::echo_openjd_directives`] — it flows all the way down from
the public API to the filter without inversion. When `true` (the default),
every recognised directive line passes through to the log
(`pass_through = true`). When `false`, recognised directives are still
parsed and acted on (progress, status, env-var changes, redacted-value
registration, …) but the lines themselves are filtered out
(`pass_through = false`). This applies uniformly to **every** directive,
including `openjd_redacted_env`. Earlier revisions of this code special-cased
`RedactedEnv` to always pass through, but that diverged from Python's
`_action_filter.py`, which routes every recognised directive through
`return not self._suppress_filtered`. The behaviour is now unified.

### Why Vec instead of Option for callbacks

A single line can produce multiple callbacks in edge cases (e.g., a malformed directive
that produces both a `Fail` callback and a cancel callback). Using `Vec`
avoids special-casing these.

### Dynamic log level via openjd_session_runtime_loglevel

The filter tracks a `log_level` (default: 20 = INFO) that can be changed at runtime
by the `openjd_session_runtime_loglevel` directive. The subprocess stdout loop checks
`filter.min_log_level()` before logging command output lines, allowing actions to
suppress verbose output by raising the level to WARNING or ERROR.

## String-Based Parsing

Directive parsing uses `str::strip_prefix` and exact string matching rather than regex:

```rust
fn parse_directive(line: &str) -> Option<(ActionMessageKind, &str)> {
    let rest = line.strip_prefix("openjd_")?;
    let colon_pos = rest.find(": ")?;
    let kind_str = &rest[..colon_pos];
    let payload = &rest[colon_pos + 2..];
    if payload.is_empty() { return None; }
    match kind_str {
        "progress" => ...,
        "status" => ...,
        "fail" => ...,
        "env" => ...,
        "redacted_env" => ...,
        "unset_env" => ...,
        "session_runtime_loglevel" => ...,
        _ => None,
    }
}
```

This is simpler and faster than regex for the fixed set of known directives. The
`openjd_` prefix and `: ` separator are checked structurally.

Additional regexes validate env var names:

```rust
static ENVVAR_SET_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^"?[A-Za-z_][A-Za-z0-9_]*=.*$"#).unwrap()
});
static ENVVAR_UNSET_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap()
});
```

### Why string matching instead of regex

Using `strip_prefix` + `find` + `match` avoids compiling a complex regex for the fixed
set of known directives. The regex approach was considered but string matching is simpler,
faster, and easier to maintain for this use case. `LazyLock` (std) is used for the env
var validation regexes which do need pattern matching.

## Malformed Command Detection

If a line looks like an `openjd_*` directive but doesn't match any known pattern (e.g.,
`openjd_ENV: FOO=bar` with wrong case, or `openjd_env:FOO=bar` missing the space), the
filter checks specifically for env-related directives:

1. Performs a case-insensitive check for `openjd_env`, `openjd_redacted_env`, or
   `openjd_unset_env` followed by a colon, space, or end-of-string
2. If it matches, emits `ActionMessage::CancelMarkFailed` with an error message
3. The session cancels the action with `Failed` state

Only env-related directives are checked for malformation because they have side effects
(setting/unsetting environment variables). Malformed `openjd_progress`, `openjd_status`,
and `openjd_fail` are silently ignored — they don't affect session state, so a false
positive would be more harmful than a missed directive.

## Redaction

### How redaction works

When `openjd_redacted_env: NAME=VALUE` is processed:

1. The value is added to `redacted_values: HashSet<String>`
2. For multiline values, the first and last lines are added to `redacted_values`,
   and middle lines are added to `redacted_lines: HashSet<String>`
3. All subsequent log lines are checked against both sets
4. Matches are replaced with `"********"` (fixed-length, 8 asterisks)

### Why fixed-length replacement

Variable-length replacement (matching the original value's length) would leak information
about the value's size. Fixed-length replacement is a security best practice for
credential redaction.

### Redaction ordering and the `echo_openjd_directives` flag

When `openjd_redacted_env: NAME=VALUE` is processed, the secret is added to
`redacted_values` *before* the originating line is considered for pass-through.
The line that the filter returns has `NAME=VALUE` rewritten to `NAME=********`
already, so it is safe to log when `echo_openjd_directives = true` (the
default). Any
later log line that happens to contain the secret value verbatim is also
redacted on the way through `apply_redaction`. This means
`echo_openjd_directives = false` is *not* a security feature — it just removes
directive lines from operator-facing output. Redaction is what protects the
secret, and it is in effect regardless of the echo flag.

### JSON-encoded env var format

Environment variable values can be JSON-encoded (e.g., `openjd_env: {"NAME": "VALUE"}`).
The filter detects JSON format and decodes it, supporting values that contain newlines
or special characters that can't be represented in the simple `NAME=VALUE` format.

## Integration with Subprocess

The subprocess stdout loop calls `filter.filter_message(line, session_id)` for each line:

```rust
let (callbacks, pass_through, modified_line) = filter.filter_message(&line, session_id);
for cb in callbacks {
    let msg = map_callback_to_message(cb);  // FilterCallback → ActionMessage
    let _ = message_tx.send(msg);
}
if pass_through {
    session_log!(info, session_id, LogContent::COMMAND_OUTPUT, "{}", modified_line);
}
```

Messages flow through the mpsc channel to `Session::drive_action()`. Non-directive lines
are logged with redaction applied.
