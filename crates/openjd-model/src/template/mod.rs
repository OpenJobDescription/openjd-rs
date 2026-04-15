// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! v2023-09 schema model types.

pub mod parse;

mod actions;
mod constrained_strings;
mod environment;
mod environment_template;
mod expr_parameters;
mod host_requirements;
mod job_template;
mod parameters;
mod step;
mod task_parameters;
pub(crate) mod validate_v2023_09;

// job_template
pub use job_template::JobTemplate;
// environment_template
pub use environment_template::EnvironmentTemplate;
// parameters
pub use parameters::JobParameterDefinition;
#[cfg(test)]
pub use parameters::{FlexFloat, FlexInt};
// step
pub use step::{SimpleAction, StepScript, StepTemplate};
// environment
pub use environment::{EmbeddedFile, Environment};
// actions
pub use actions::{Action, CancelationMode};
// host_requirements
pub use host_requirements::HostRequirements;
// task_parameters
pub use task_parameters::{
    FloatRange, FloatRangeItem, IntOrFormatString, IntRange, RangeConstraint,
    StepParameterSpaceDefinition, StringRange, TaskParameterDefinition,
};
// constrained_strings
#[cfg(test)]
pub use constrained_strings::Identifier;
// expr_parameters
#[cfg(test)]
pub use expr_parameters::{
    JobBoolParameterDefinition, JobListBoolParameterDefinition, JobListFloatParameterDefinition,
    JobListIntParameterDefinition, JobListListIntParameterDefinition,
    JobListPathParameterDefinition, JobListStringParameterDefinition,
    JobRangeExprParameterDefinition, ListFloatItemConstraints, ListIntItemConstraints,
    ListListIntItemConstraints, ListStringItemConstraints,
};
