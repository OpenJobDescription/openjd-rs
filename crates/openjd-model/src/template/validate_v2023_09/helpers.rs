// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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

/// Which kind of `hostRequirements` entry a capability name belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapabilityKind {
    Amount,
    Attribute,
}

impl CapabilityKind {
    /// The `hostRequirements` field the entry lives in.
    pub fn field(self) -> &'static str {
        match self {
            Self::Amount => "amounts",
            Self::Attribute => "attributes",
        }
    }

    /// The noun the duplicate-name message uses.
    pub fn noun(self) -> &'static str {
        match self {
            Self::Amount => "amount",
            Self::Attribute => "attribute",
        }
    }

    /// The §3.3.1.1 / §3.3.2.1 name pattern.
    pub fn pattern(self) -> &'static Regex {
        match self {
            Self::Amount => &AMOUNT_CAP_RE,
            Self::Attribute => &ATTR_CAP_RE,
        }
    }
}

/// Check the §3.3.1.1 / §3.3.2.1 constraints on a capability name whose
/// value is known: its length, its pattern, and its reserved scope.
///
/// A capability name is `@fmtstring`, and its constraints apply to the
/// resolved name. Template validation calls this for a literal name and for
/// a name that is fully static, and job creation calls it for the resolved
/// name, so a violation reads the same way whichever stage caught it.
/// `standard` holds the standard capability names for `kind`.
pub fn check_capability_name(
    name: &str,
    kind: CapabilityKind,
    standard: &[&str],
    path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    if name.chars().count() > 100 {
        errors.add(path, format!("name '{name}' exceeds 100 characters."));
    }
    if !kind.pattern().is_match(name) {
        errors.add(
            path,
            format!("name '{name}' does not match capability name pattern."),
        );
    }
    check_capability_reserved_scope(name, standard, path, errors);
}

/// Check the rule that `allOf` on the single-valued standard attributes
/// (`attr.worker.os.family`, `attr.worker.cpu.arch`) has at most one
/// element, for an attribute whose name is known.
///
/// Counted on the elements that certainly contribute exactly one resolved
/// element: literals, and multi-segment format strings, which always
/// concatenate to a single string (Expression Language §1.3.2). Only a
/// whole-field single-expression element can null-skip or list-flatten, so
/// its contribution is unknowable here and job creation re-checks the
/// resolved count. Two or more certain elements violate the rule under
/// every possible resolution. `attr_path` is the attribute's path.
pub fn check_single_valued_all_of(
    capability_name: &str,
    all_of: Option<&[openjd_expr::FormatString]>,
    attr_path: &[PathElement],
    errors: &mut ValidationErrors,
) {
    let lower = capability_name.to_lowercase();
    if lower != "attr.worker.os.family" && lower != "attr.worker.cpu.arch" {
        return;
    }
    let Some(vals) = all_of else {
        return;
    };
    let certain = |v: &openjd_expr::FormatString| v.is_literal() || v.segment_count() > 1;
    if vals.iter().filter(|v| certain(v)).count() > 1 {
        errors.add(
            &crate::error::path_field(attr_path, "allOf"),
            "single-valued attribute cannot have more than 1 element.",
        );
    }
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
    if name.chars().count() > 256 {
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
