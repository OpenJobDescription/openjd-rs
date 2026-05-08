// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Open Job Description model library for Rust.
//!
//! Provides parsing, validation, and job creation for OpenJD templates
//! conforming to the 2023-09 specification.

pub mod error;
pub mod job;
pub(crate) mod template;
pub use job::create_job;
pub use job::step_dependency_graph;
pub use job::step_param_space;
pub use template::parse;
pub use template::{EnvironmentTemplate, JobParameterDefinition, JobTemplate};
pub mod capabilities;
pub mod types;

// Re-export FormatString and SymbolTable from openjd-expr.
pub use openjd_expr::format_string;
pub use openjd_expr::format_string::FormatString;
pub use openjd_expr::symbol_table;
pub use openjd_expr::symbol_table::SymbolTable;

#[cfg(test)]
mod test_lazy_param_space;

pub use error::ModelError;
pub use error::{DiagnosticSpan, ErrorDetail};
pub use error::{PathElement, ValidationError, ValidationErrors};
pub use job::create_job::{
    build_symbol_table, convert_environment, create_job, evaluate_let_bindings,
    merge_job_parameter_definitions, preprocess_job_parameters, MergedParameterDefinition,
    PathParameterOptions,
};
pub use parse::{
    decode_environment_template, decode_job_template, decode_template, DecodedTemplate,
    DocumentType,
};
pub use step_dependency_graph::StepDependencyGraph;
pub use step_param_space::StepParameterSpaceIterator;
pub use template::TaskParameterDefinition;
pub use types::{
    CallerLimits, DataFlow, EndOfLine, Extensions, FileType, JobParameterInputValues,
    JobParameterType, JobParameterValue, JobParameterValues, ModelExtension, ModelProfile,
    ObjectType, SpecificationRevision, TaskParameterSet, TaskParameterType, TaskParameterValue,
    TemplateSpecificationVersion, ValidationContext,
};

#[cfg(test)]
mod test_expr_param_constraints;
#[cfg(test)]
mod test_instantiate_and_display;
