// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Environment template per spec §1.2.

use super::constrained_strings::ExtensionName;
use super::environment::Environment;
use super::parameters::JobParameterDefinition;
use serde::Deserialize;

/// §1.2 EnvironmentTemplate
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvironmentTemplate {
    pub specification_version: String,
    pub extensions: Option<Vec<ExtensionName>>,
    pub parameter_definitions: Option<Vec<JobParameterDefinition>>,
    pub environment: Environment,
}

impl EnvironmentTemplate {
    pub fn environment(&self) -> &Environment {
        &self.environment
    }
}
