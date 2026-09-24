// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Host requirements per spec §3.3.

use crate::format_string::FormatString;
use serde::Deserialize;

/// §3.3 HostRequirements
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostRequirements {
    pub amounts: Option<Vec<AmountRequirement>>,
    pub attributes: Option<Vec<AttributeRequirement>>,
}

/// §3.3.1 AmountRequirement
///
/// `name` is `@fmtstring`: a name containing expressions is resolved, and
/// its §3.3.1.1 constraints checked, at job creation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AmountRequirement {
    pub name: FormatString,
    pub min: Option<FormatString>,
    pub max: Option<FormatString>,
}

/// §3.3.2 AttributeRequirement
///
/// `name` is `@fmtstring`: a name containing expressions is resolved, and
/// its §3.3.2.1 constraints checked, at job creation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttributeRequirement {
    pub name: FormatString,
    pub any_of: Option<Vec<FormatString>>,
    pub all_of: Option<Vec<FormatString>>,
}
