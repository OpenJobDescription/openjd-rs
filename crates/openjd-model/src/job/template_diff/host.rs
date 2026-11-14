// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Host support: template knowledge a job store needs beside a plan.
//!
//! **Unstable.** This API may change or be removed in any release without
//! notice. Don't depend on it outside openjd-rs.
//!
//! A store that keeps a job as rows and bodies needs a few things that
//! only template knowledge can answer: the raw template fragment of a step
//! or environment, the job-scope symbol table, a stable digest of a built
//! job, and which stored parameter values carry over to a new template.
//! See `specs/model/host-support.md`.

use std::fmt::Write as _;

use openjd_expr::symbol_table::SerializedSymbolTable;
use openjd_expr::ExprValue;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{keepable, load, typed_values, TemplateInput};
use crate::create_job::build_symbol_table;
use crate::error::ModelError;
use crate::job::template_diff::JobDocument;
use crate::job::Job;
use crate::types::{
    JobParameterInputValues, JobParameterValue, JobParameterValues, ModelExtension,
};

/// A step of a template document, as the document holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateStepFragment {
    /// The step's object, every field as written (format strings
    /// unresolved), including `stepEnvironments`.
    pub step: Value,
    /// The step's `stepEnvironments`, in order; empty when it has none.
    pub step_environments: Vec<Value>,
}

fn fragment_error(message: String) -> ModelError {
    ModelError::DecodeValidation(message)
}

/// The object named `name` in the document's list `list`.
fn named<'a>(
    document: &'a Value,
    list: &str,
    name: &str,
    what: &str,
) -> Result<&'a Value, ModelError> {
    let items = document
        .get(list)
        .and_then(Value::as_array)
        .ok_or_else(|| fragment_error(format!("the template document has no '{list}' list")))?;
    items
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| fragment_error(format!("the template has no {what} named '{name}'")))
}

/// The step named `step` in a job template document, and its step
/// environments, as the document holds them: before decoding, so format
/// strings are unresolved and nothing is defaulted.
///
/// `document` is the template's parsed document, the value a host passes to
/// [`decode_job_template`](crate::decode_job_template). The decoded
/// [`JobTemplate`](crate::template::JobTemplate) is not enough: its types do not serialize back to the
/// document. A host that stores a step body without some fields (such as
/// `stepEnvironments` or `parameterSpace`) removes them itself.
pub fn template_step_fragment(
    document: &Value,
    step: &str,
) -> Result<TemplateStepFragment, ModelError> {
    let found = named(document, "steps", step, "step")?;
    let step_environments = match found.get("stepEnvironments") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(envs)) => envs.clone(),
        Some(_) => {
            return Err(fragment_error(format!(
                "the 'stepEnvironments' of step '{step}' is not a list"
            )))
        }
    };
    Ok(TemplateStepFragment {
        step: found.clone(),
        step_environments,
    })
}

/// The job environment named `name` in a job template document, as the
/// document holds it. See [`template_step_fragment`] for `document`.
pub fn template_job_environment_fragment(
    document: &Value,
    name: &str,
) -> Result<Value, ModelError> {
    named(document, "jobEnvironments", name, "job environment").cloned()
}

/// The job-scope symbol table of a built job, in transport form: `Param.*`
/// and `RawParam.*` for every parameter (a `PATH` or `LIST[PATH]` parameter as
/// `RawParam.*` only), and `Job.Name` when the job declares the `EXPR`
/// extension. It is the table `create_job` resolves job-scope format strings
/// against, built with [`build_symbol_table`]; each environment's
/// [`resolved_symtab`](crate::job::Environment::resolved_symtab) is this
/// table filtered to the symbols the environment reads.
///
/// `Job.Name` follows the job's declared extensions, as it does when the job
/// is built with the template's own validation context, which
/// [`TemplateInput`](super::TemplateInput) uses.
pub fn build_job_symbol_table(job: &Job) -> Result<SerializedSymbolTable, ModelError> {
    let values: JobParameterValues = job
        .parameters
        .iter()
        .map(|(name, p)| {
            (
                name.clone(),
                JobParameterValue {
                    param_type: p.param_type,
                    value: p.value.clone(),
                },
            )
        })
        .collect();
    let mut table = build_symbol_table(&values)?;
    if job
        .extensions
        .iter()
        .flatten()
        .any(|e| *e == ModelExtension::Expr)
    {
        table.set("Job.Name", ExprValue::String(job.name.clone()))?;
    }
    Ok(SerializedSymbolTable::from_symtab(&table))
}

/// A stable digest of a built job: the lowercase hex SHA-256 of its
/// [`JobDocument`] JSON text. The document writes values in the host
/// encoding and sorts its maps, so value-equal jobs have the same digest, in
/// any process. The exception is a float, whose document keeps its literal:
/// `1.0` and `1.00` compare equal but digest differently. Any change
/// [`diff_jobs`](crate::job::diff::diff_jobs) reports changes the
/// document, and so the digest. The converse does not hold: a document
/// change the diff ignores (a task parameter definition reorder under an
/// explicit `combination`) also changes the digest. `==` jobs whose task
/// parameter definitions are in a different order also digest differently,
/// because the document keeps definition order, which without an explicit
/// `combination` is the product's dimension order. The document includes its
/// `documentVersion`, so digests compare only within one version.
pub fn job_digest(job: &Job) -> Result<String, ModelError> {
    let text = serde_json::to_vec(&JobDocument(job))
        .map_err(|e| ModelError::DecodeValidation(format!("cannot write the job as data: {e}")))?;
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(&text) {
        // Writing to a `String` cannot fail.
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex)
}

/// The values to build a job from a new template with, given the job's
/// stored (preprocessed) values and the values a caller requests, ready for
/// [`TemplateInput::preprocessed`](super::TemplateInput::preprocessed):
///
/// - a parameter the caller leaves out keeps its stored value, untouched,
///   when the new template (with `new`'s environments) still declares it
///   with the same type;
/// - every other value, requested or a new default, is preprocessed against
///   the new template with `new`'s directories, as `new` builds.
///
/// `new` is the side the values are for. Its source, environments and
/// directories are used; its own parameter values are not. A requested
/// value the new template does not declare, or a required parameter that
/// is neither requested nor kept, is preprocessing's error. A kept value is
/// neither preprocessed again nor checked here: preprocessing could rewrite
/// it (a Windows PATH on a POSIX host is relative), and the rewritten form
/// could fail a constraint the stored value meets. It counts as supplied,
/// so a required parameter is not missing. It is checked, as stored,
/// against every definition that declares it (the job template's and its
/// environment templates') when the job is built from it.
pub fn merge_parameters(
    stored: &JobParameterValues,
    requested: &JobParameterInputValues,
    new: &TemplateInput,
) -> Result<JobParameterValues, ModelError> {
    let loaded = load(new)?;
    let kept = keepable(&loaded, stored, requested)?;
    typed_values(&loaded, kept, requested)
}
