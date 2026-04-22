// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

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
