// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Environment types per spec §4.

use super::actions::EnvironmentActions;
use super::constrained_strings::Description;
use crate::format_string::FormatString;
use crate::types::{EndOfLine, FileType};
use serde::Deserialize;
use std::collections::HashMap;

/// §4 Environment
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Environment {
    pub name: String,
    pub description: Option<Description>,
    pub script: Option<EnvironmentScript>,
    pub variables: Option<HashMap<String, FormatString>>,
}

/// §4.1 EnvironmentScript
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentScript {
    #[serde(rename = "let")]
    pub let_bindings: Option<Vec<String>>,
    pub actions: EnvironmentActions,
    pub embedded_files: Option<Vec<EmbeddedFile>>,
}

/// §6 EmbeddedFile
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddedFile {
    pub name: String,
    #[serde(rename = "type")]
    pub file_type: FileType,
    pub filename: Option<String>,
    pub data: Option<FormatString>,
    pub runnable: Option<bool>,
    #[serde(rename = "endOfLine")]
    pub end_of_line: Option<EndOfLine>,
}
