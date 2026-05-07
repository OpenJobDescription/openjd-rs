// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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

    /// Build a [`ValidationContext`](crate::types::ValidationContext)
    /// matching this template: revision derived from
    /// `specificationVersion`, extensions populated from the template's
    /// declared `extensions` list (ignoring entries that don't parse as
    /// a [`KnownExtension`](crate::types::KnownExtension)), and caller
    /// limits left at their defaults.
    ///
    /// This is the convenient "do what the template says" context for
    /// callers that do not want to override revision/extension policy.
    /// Callers that *do* want to override (e.g. a service stripping EXPR
    /// regardless of template intent) should build a
    /// `ValidationContext` explicitly and use
    /// [`with_caller_limits`](crate::types::ValidationContext::with_caller_limits)
    /// as needed.
    pub fn default_validation_context(&self) -> crate::types::ValidationContext {
        use std::str::FromStr;
        let revision =
            crate::types::TemplateSpecificationVersion::from_str(&self.specification_version)
                .map(|v| v.revision())
                // Unknown spec versions shouldn't reach this point (the template
                // was validated). Fall back to the first revision.
                .unwrap_or(crate::types::SpecificationRevision::V2023_09);
        let mut exts = crate::types::Extensions::new();
        if let Some(list) = &self.extensions {
            for e in list {
                if let Ok(known) = crate::types::KnownExtension::from_str(e.as_str()) {
                    exts.insert(known);
                }
            }
        }
        crate::types::ValidationContext::with_extensions(revision, exts)
    }
}
