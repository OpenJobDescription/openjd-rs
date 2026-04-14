// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Standard capability definitions for the Open Job Description specification.

/// Standard amount capabilities and their names.
pub const STANDARD_AMOUNT_CAPABILITIES: &[&str] = &[
    "amount.worker.vcpu",
    "amount.worker.memory",
    "amount.worker.gpu",
    "amount.worker.gpu.memory",
    "amount.worker.disk.scratch",
];

/// Standard attribute capabilities, their names, and allowed values.
pub const STANDARD_ATTRIBUTE_CAPABILITIES: &[(&str, &[&str])] = &[
    ("attr.worker.os.family", &["linux", "windows", "macos"]),
    ("attr.worker.cpu.arch", &["x86_64", "arm64"]),
];

/// Standard attribute capability names (without values).
pub const STANDARD_ATTRIBUTE_CAPABILITY_NAMES: &[&str] =
    &["attr.worker.os.family", "attr.worker.cpu.arch"];

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
