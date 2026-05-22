// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

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
pub(crate) mod validation;

// job_template
pub use job_template::JobTemplate;
// environment_template
pub use environment_template::EnvironmentTemplate;
// parameters
pub use parameters::{
    FileFilter, FloatUserInterface, IntUserInterface, JobFloatParameterDefinition,
    JobIntParameterDefinition, JobParameterDefinition, JobPathParameterDefinition,
    JobStringParameterDefinition, PathUserInterface, StringUserInterface,
};
pub use parameters::{FlexFloat, FlexInt};
// expr_parameters (EXPR-extension job parameter types)
pub use expr_parameters::{
    BoolUserInterface, HiddenOnlyUserInterface, JobBoolParameterDefinition,
    JobListBoolParameterDefinition, JobListFloatParameterDefinition, JobListIntParameterDefinition,
    JobListListIntParameterDefinition, JobListPathParameterDefinition,
    JobListStringParameterDefinition, JobRangeExprParameterDefinition, ListFloatUserInterface,
    ListIntUserInterface, ListPathUserInterface, ListSimpleUserInterface, RangeExprUserInterface,
};
#[cfg(test)]
pub use expr_parameters::{
    ListFloatItemConstraints, ListIntItemConstraints, ListListIntItemConstraints,
    ListStringItemConstraints,
};
// step
pub use step::{SimpleAction, StepDependency, StepScript, StepTemplate};
// environment
pub use environment::{EmbeddedFile, Environment, EnvironmentScript};
// actions
pub use actions::{Action, CancelationMode, EnvironmentActions, StepActions};
// host_requirements
pub use host_requirements::{AmountRequirement, AttributeRequirement, HostRequirements};
// task_parameters
pub use task_parameters::{
    ChunkIntTaskParameterDefinition, ChunksDefinition, FloatRange, FloatRangeItem,
    FloatTaskParameterDefinition, IntOrFormatString, IntRange, IntTaskParameterDefinition,
    PathTaskParameterDefinition, RangeConstraint, StepParameterSpaceDefinition, StringRange,
    StringTaskParameterDefinition, TaskParameterDefinition,
};
// constrained_strings
#[cfg(test)]
pub use constrained_strings::Identifier;
pub use constrained_strings::{Description, ExtensionName};
