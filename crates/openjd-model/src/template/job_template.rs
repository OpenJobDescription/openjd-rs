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

    /// Derive the [`ModelProfile`](crate::ModelProfile) described by
    /// this template: the revision from `specificationVersion` and the
    /// extensions set declared on the template.
    ///
    /// Entries in the `extensions` list that don't parse as a known
    /// [`ModelExtension`](crate::types::ModelExtension) are silently
    /// skipped.
    ///
    /// This is the "what the template says it needs" profile — the one
    /// to pass to sessions, to
    /// [`ModelProfile::to_expr_profile`](crate::ModelProfile::to_expr_profile),
    /// or to wrap in a
    /// [`ValidationContext`](crate::types::ValidationContext) when
    /// calling `create_job`.
    pub fn profile(&self) -> crate::ModelProfile {
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
                if let Ok(known) = crate::types::ModelExtension::from_str(e.as_str()) {
                    exts.insert(known);
                }
            }
        }
        crate::ModelProfile::new(revision).with_extensions(exts)
    }

    /// Convenience: wrap [`profile`](Self::profile) in a
    /// [`ValidationContext`](crate::types::ValidationContext) with
    /// default caller limits. Equivalent to
    /// `ValidationContext::from_profile(self.profile())`.
    ///
    /// This is the convenient "do what the template says" context for
    /// callers that do not want to override revision/extension policy.
    /// Callers that *do* want to override (e.g. a service stripping EXPR
    /// regardless of template intent) should build a
    /// `ValidationContext` explicitly and use
    /// [`with_caller_limits`](crate::types::ValidationContext::with_caller_limits)
    /// as needed.
    pub fn default_validation_context(&self) -> crate::types::ValidationContext {
        crate::types::ValidationContext::from_profile(self.profile())
    }
}
