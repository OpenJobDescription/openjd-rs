// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared validation helpers.

use regex::Regex;
use std::sync::LazyLock;

use crate::error::{PathElement, ValidationErrors};

pub static AMOUNT_CAP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^([A-Za-z_][A-Za-z0-9_]*:)?amount\.[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$").unwrap()
});
pub static ATTR_CAP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^([A-Za-z_][A-Za-z0-9_]*:)?attr\.[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)*$",
    )
    .unwrap()
});
pub static ATTR_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_\-]*$").unwrap());

pub const RESERVED_SCOPES: &[&str] = &["worker", "job", "step", "task"];

pub fn has_control_chars(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
}

/// Check if a capability name uses a reserved scope without being a standard capability.
pub fn check_capability_reserved_scope(
    name: &str,
    standard: &[&str],
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let lower = name.to_lowercase();
    let capability = if lower.contains(':') {
        lower.split(':').nth(1).unwrap_or(&lower)
    } else {
        &lower
    };
    if standard.contains(&capability) {
        return;
    }
    let parts: Vec<&str> = capability.split('.').collect();
    if parts.len() >= 2 {
        let scope = parts[1];
        if RESERVED_SCOPES.contains(&scope) {
            errors.add(path, format!("capability '{name}' uses reserved scope '{scope}'. Only spec-defined capabilities may use this scope."));
        }
    }
}

/// Validate an environment variable name.
pub fn validate_env_var_name(name: &str, path: &[PathElement], errors: &mut ValidationErrors) {
    if name.is_empty() {
        errors.add(path, "variable name must not be empty.");
        return;
    }
    if name.len() > 256 {
        errors.add(
            path,
            format!("variable name '{name}' exceeds 256 characters."),
        );
    }
    let first = name.chars().next().unwrap();
    if first.is_ascii_digit() {
        errors.add(
            path,
            format!("variable name '{name}' cannot start with a digit."),
        );
    }
    for ch in name.chars() {
        if !ch.is_ascii_alphanumeric() && ch != '_' {
            errors.add(
                path,
                format!("variable name '{name}' contains invalid character '{ch}'."),
            );
            return;
        }
    }
}
