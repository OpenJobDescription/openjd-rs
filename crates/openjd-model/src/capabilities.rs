// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Standard capability definitions for the Open Job Description specification.
//!
//! Capability lookup functions take `(SpecificationRevision, &Extensions)` and
//! return `Result` because the set of standard capabilities may vary by revision,
//! and future extensions may add or modify capabilities that are only valid for
//! certain revision+extension combinations.

use crate::error::ModelError;
use crate::types::{Extensions, SpecificationRevision};

/// Standard amount capabilities and their names.
const STANDARD_AMOUNT_CAPABILITIES_V2023_09: &[&str] = &[
    "amount.worker.vcpu",
    "amount.worker.memory",
    "amount.worker.gpu",
    "amount.worker.gpu.memory",
    "amount.worker.disk.scratch",
];

/// Standard attribute capabilities, their names, and allowed values.
const STANDARD_ATTRIBUTE_CAPABILITIES_V2023_09: &[(&str, &[&str])] = &[
    ("attr.worker.os.family", &["linux", "windows", "macos"]),
    ("attr.worker.cpu.arch", &["x86_64", "arm64"]),
];

fn unsupported_revision(revision: SpecificationRevision) -> ModelError {
    ModelError::DecodeValidation(format!("Unsupported specification revision: {revision}"))
}

/// Return the standard amount capability names for the given revision and extensions.
pub fn standard_amount_capability_names(
    revision: SpecificationRevision,
    _extensions: &Extensions,
) -> Result<&'static [&'static str], ModelError> {
    #[allow(unreachable_patterns)] // wildcard needed for forward compat with new revisions
    match revision {
        SpecificationRevision::V2023_09 => Ok(STANDARD_AMOUNT_CAPABILITIES_V2023_09),
        _ => Err(unsupported_revision(revision)),
    }
}

/// Return the standard attribute capability names for the given revision and extensions.
pub fn standard_attribute_capability_names(
    revision: SpecificationRevision,
    _extensions: &Extensions,
) -> Result<Vec<&'static str>, ModelError> {
    #[allow(unreachable_patterns)]
    match revision {
        SpecificationRevision::V2023_09 => Ok(STANDARD_ATTRIBUTE_CAPABILITIES_V2023_09
            .iter()
            .map(|(name, _)| *name)
            .collect()),
        _ => Err(unsupported_revision(revision)),
    }
}

/// Return the standard attribute capabilities (name + allowed values) for the given
/// revision and extensions.
pub fn standard_attribute_capabilities(
    revision: SpecificationRevision,
    _extensions: &Extensions,
) -> Result<&'static [(&'static str, &'static [&'static str])], ModelError> {
    #[allow(unreachable_patterns)]
    match revision {
        SpecificationRevision::V2023_09 => Ok(STANDARD_ATTRIBUTE_CAPABILITIES_V2023_09),
        _ => Err(unsupported_revision(revision)),
    }
}

/// Validate that a string is a valid amount capability name.
pub fn validate_amount_capability_name(name: &str) -> Result<(), String> {
    let re = &crate::template::validate_v2023_09::helpers::AMOUNT_CAP_RE;
    if re.is_match(name) {
        Ok(())
    } else {
        Err(format!("'{name}' is not a valid amount capability name"))
    }
}

/// Validate that a string is a valid attribute capability name.
pub fn validate_attribute_capability_name(name: &str) -> Result<(), String> {
    let re = &crate::template::validate_v2023_09::helpers::ATTR_CAP_RE;
    if re.is_match(name) {
        Ok(())
    } else {
        Err(format!("'{name}' is not a valid attribute capability name"))
    }
}

/// Validate a single `<AttributeCapabilityValue>` (§3.3.2.2) for the named
/// attribute capability.
///
/// A standard capability constrains its values to a fixed set, compared
/// case-insensitively, and nothing else — the identifier pattern and length
/// limit do not apply to it. Any other capability constrains its values to the
/// identifier-like pattern with a 100 character limit. That split is exclusive
/// to match the reference implementation, which likewise checks a standard
/// capability against its value set only.
///
/// `standard_capabilities` is the table from [`standard_attribute_capabilities`]
/// for the caller's revision and extensions.
///
/// Messages mirror the wording the decode-time check produces for the same
/// violation on a literal value, with one deliberate difference: the length
/// message names the resolved value. A template that reached this function
/// through a format string does not show that value anywhere, so omitting it
/// would leave the user nothing to act on.
pub fn validate_attribute_capability_value(
    capability_name: &str,
    value: &str,
    standard_capabilities: &[(&str, &[&str])],
) -> Result<(), String> {
    let lower = capability_name.to_lowercase();
    if let Some((name, allowed)) = standard_capabilities.iter().find(|(n, _)| *n == lower) {
        if allowed.iter().any(|v| v.eq_ignore_ascii_case(value)) {
            Ok(())
        } else {
            Err(format!("value '{value}' is not valid for {name}."))
        }
    } else if value.is_empty() {
        Err("must not be empty.".to_string())
    } else if value.len() > 100 {
        Err(format!("value '{value}' exceeds 100 characters."))
    } else if crate::template::validate_v2023_09::helpers::ATTR_VALUE_RE.is_match(value) {
        Ok(())
    } else {
        Err(format!("value '{value}' contains invalid characters."))
    }
}
