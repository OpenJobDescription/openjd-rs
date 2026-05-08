// Copyright by contributors to this project.
// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Revision-neutral entry points to template validation.
//!
//! The validation pipeline is split into two layers:
//!
//! 1. **This module (`template::validation`)** holds the revision-neutral
//!    interface — the [`EffectiveLimits`] and [`EffectiveRules`] types
//!    that parameterise the passes, and the top-level
//!    [`validate_job_template`] / [`validate_environment_template`]
//!    functions that dispatch on the specification revision carried in
//!    the [`ValidationContext`].
//!
//! 2. **Revision-specific submodules** (today: `validate_v2023_09`,
//!    re-exported via this module's `v2023_09` alias) hold the pass
//!    implementations that know about a particular revision's template
//!    shape. Future revisions (e.g. `validate_v2027_xx`) will sit
//!    alongside, share the revision-neutral interface types, and plug
//!    into the dispatch arms in this file.
//!
//! The dispatch arms currently have a single revision, but making the
//! dispatch explicit now is item 9 of the future-revision readiness
//! report: it localizes the one-line change needed when a second
//! revision ships.

use crate::error::ModelError;
use crate::template::{EnvironmentTemplate, JobTemplate};
use crate::types::{SpecificationRevision, ValidationContext};

// Revision-neutral types and the existing v2023_09 pass implementations
// still live in `validate_v2023_09` for now. Re-export the types so
// consumers of this module (including the decode layer in
// `template::parse`) have a single, revision-neutral import path.
#[allow(unused_imports)] // re-exported as part of the public interface of this module
pub use crate::template::validate_v2023_09::{EffectiveLimits, EffectiveRules};

/// Validate a job template, dispatching to the per-revision pipeline.
pub(crate) fn validate_job_template(
    jt: &JobTemplate,
    ctx: &ValidationContext,
) -> Result<(), ModelError> {
    match ctx.profile.revision() {
        SpecificationRevision::V2023_09 => {
            crate::template::validate_v2023_09::validate_job_template(jt, ctx)
        }
    }
}

/// Validate an environment template, dispatching to the per-revision
/// pipeline.
pub fn validate_environment_template(
    et: &EnvironmentTemplate,
    ctx: &ValidationContext,
) -> Result<(), ModelError> {
    match ctx.profile.revision() {
        SpecificationRevision::V2023_09 => {
            crate::template::validate_v2023_09::validate_environment_template(et, ctx)
        }
    }
}
