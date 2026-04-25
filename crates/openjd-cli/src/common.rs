// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared utilities for CLI commands.

use std::fmt;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "TASK_CHUNKING",
    "REDACTED_ENV_VARS",
    "FEATURE_BUNDLE_1",
    "EXPR",
];

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
                .filter(|e| !SUPPORTED_EXTENSIONS.contains(&e.as_str()))
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
        None => Ok(SUPPORTED_EXTENSIONS.iter().map(|s| s.to_string()).collect()),
    }
}
