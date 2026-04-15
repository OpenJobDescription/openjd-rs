// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Job template per spec §1.1.

use super::constrained_strings::{Description, ExtensionName};
use super::environment::Environment;
use super::parameters::JobParameterDefinition;
use super::step::StepTemplate;
use crate::format_string::FormatString;
use serde::Deserialize;

/// §1.1 JobTemplate
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobTemplate {
    pub specification_version: String,
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
    pub extensions: Option<Vec<ExtensionName>>,
    pub name: FormatString,
    pub description: Option<Description>,
    pub parameter_definitions: Option<Vec<JobParameterDefinition>>,
    pub job_environments: Option<Vec<Environment>>,
    pub steps: Vec<StepTemplate>,
}

impl JobTemplate {
    pub fn name(&self) -> &FormatString {
        &self.name
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_ref().map(|d| d.0.as_str())
    }

    pub fn parameter_definitions_list(&self) -> &[JobParameterDefinition] {
        match &self.parameter_definitions {
            Some(defs) => defs,
            None => &[],
        }
    }
}
