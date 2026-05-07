// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Shared utilities for CLI commands.

use std::fmt;
use std::io::Read;
use std::path::Path;

/// Maximum size, in bytes, of a `file://`-referenced input read by the CLI
/// (job parameter files, task files, path-mapping rule files).
///
/// This bounds memory use when the CLI is invoked programmatically with
/// `file://` paths from less-trusted sources. The template itself is bounded
/// separately by the model crate's `CallerLimits::max_template_size`.
///
/// 10 MiB is large enough for any reasonable parameter / task / path-mapping
/// document and small enough to prevent runaway allocation.
pub const MAX_FILE_INPUT_SIZE: u64 = 10 * 1024 * 1024;

/// Read a file's contents as a UTF-8 string, mapping I/O errors to descriptive
/// CLI error messages and optionally enforcing a maximum file size.
///
/// When `max_size` is `Some(cap)`, reads at most `cap + 1` bytes via
/// [`Read::take`]; if the resulting buffer exceeds `cap` the file is
/// rejected with an "exceeds maximum size" error. Allocation is bounded
/// to `cap + 1` bytes regardless of how large the underlying file is.
///
/// No separate existence / type check precedes the read — the open itself
/// is the only filesystem access on the happy path, eliminating TOCTOU
/// between a check and the read. A best-effort `metadata()` call on the
/// failed-open path is used *only* to refine the error message (e.g. to
/// distinguish a directory from a generic I/O error); it is not a security
/// control.
///
/// Error messages:
///
/// * NotFound → `not_found_msg()`
/// * Directory → `"'<path>' is not a file."`
/// * Exceeds `max_size` → `"File '<path>' exceeds maximum size of <N> bytes."`
/// * Any other open / read error → `read_err_prefix()` followed by `": <source>"`
pub fn read_input_file_with<FN, FE>(
    path: &Path,
    max_size: Option<u64>,
    not_found_msg: FN,
    read_err_prefix: FE,
) -> Result<String, String>
where
    FN: FnOnce() -> String,
    FE: FnOnce() -> String,
{
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_found_msg()),
        Err(e) => {
            // Best-effort: a failed open on a directory typically reports
            // IsADirectory on Linux but Other on some platforms, so use
            // metadata() to produce a stable diagnostic.
            if std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false) {
                return Err(format!("'{}' is not a file.", path.display()));
            }
            return Err(format!("{}: {e}", read_err_prefix()));
        }
    };

    // On Linux, File::open succeeds on a directory and the read fails with
    // IsADirectory. Detect that up-front via the open file's metadata so we
    // produce the same "is not a file" message on every platform.
    if file.metadata().map(|m| m.is_dir()).unwrap_or(false) {
        return Err(format!("'{}' is not a file.", path.display()));
    }

    let mut buf = String::new();
    match max_size {
        Some(cap) => {
            // Read cap + 1 bytes. If the file is larger than the cap,
            // we'll have read cap + 1 bytes; if it's cap or fewer, we'll
            // have read the whole file. Bounded allocation either way.
            let limit = cap.saturating_add(1);
            file.take(limit)
                .read_to_string(&mut buf)
                .map_err(|e| format!("{}: {e}", read_err_prefix()))?;
            if buf.len() as u64 > cap {
                return Err(format!(
                    "File '{}' exceeds maximum size of {} bytes.",
                    path.display(),
                    cap
                ));
            }
        }
        None => {
            let mut file = file;
            file.read_to_string(&mut buf)
                .map_err(|e| format!("{}: {e}", read_err_prefix()))?;
        }
    }
    Ok(buf)
}

/// Convenience wrapper for template-style reads: no size cap and the standard
/// `'<path>' does not exist.` / `Cannot read '<path>'` messages used by the
/// `check`, `summary`, `run`, and `help` subcommands.
pub fn read_input_file(path: &Path) -> Result<String, String> {
    read_input_file_with(
        path,
        None,
        || format!("'{}' does not exist.", path.display()),
        || format!("Cannot read '{}'", path.display()),
    )
}

/// The full default list of supported OpenJD extension names,
/// sourced from the authoritative [`openjd_model::KnownExtension::ALL`].
/// Using the enum rather than a CLI-local string array keeps the
/// CLI, JS bindings, and Python bindings in sync whenever a new
/// extension is added upstream — one place to update.
fn supported_extensions() -> Vec<&'static str> {
    openjd_model::KnownExtension::ALL
        .iter()
        .map(|e| e.as_str())
        .collect()
}

/// A CLI command result that can be rendered as JSON, YAML, or human-readable text.
///
/// This mirrors Python's `OpenJDCliResult` / `@print_cli_result` pattern:
/// each command returns a result struct, and output formatting is handled
/// uniformly by [`print_cli_result`].
pub trait CliResult: fmt::Display {
    /// Serialize to a JSON value for structured output.
    ///
    /// Fields with `None` / empty values should be omitted to match
    /// Python's `_asdict_omit_null` behavior.
    fn to_json_value(&self) -> serde_json::Value;
}

/// Format and print a [`CliResult`] according to the user's `--output` choice.
///
/// Equivalent to Python's `@print_cli_result` decorator.
pub fn print_cli_result(result: &dyn CliResult, format: &str) {
    match format {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&result.to_json_value()).unwrap()
        ),
        "yaml" => print!(
            "{}",
            serde_saphyr::to_string(&result.to_json_value()).unwrap()
        ),
        _ => println!("{result}"),
    }
}

/// Parse and validate the `--extensions` argument.
/// Returns an error if any extension name is not recognized.
pub fn parse_extensions(arg: &Option<String>) -> Result<Vec<String>, String> {
    let supported = supported_extensions();
    match arg {
        Some(ext_str) if ext_str.is_empty() => Ok(vec![]),
        Some(ext_str) => {
            let exts: Vec<String> = ext_str
                .split(',')
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty())
                .collect();
            let unsupported: Vec<&str> = exts
                .iter()
                .filter(|e| !supported.contains(&e.as_str()))
                .map(|e| e.as_str())
                .collect();
            if !unsupported.is_empty() {
                return Err(format!(
                    "Unsupported Open Job Description extension(s): {}",
                    unsupported.join(", ")
                ));
            }
            Ok(exts)
        }
        None => Ok(supported.iter().map(|s| s.to_string()).collect()),
    }
}
