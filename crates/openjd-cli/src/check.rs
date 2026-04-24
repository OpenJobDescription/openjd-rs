// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! `openjd-rs check` command — validate a template file.

use clap::Args;
use openjd_model::parse::{self, DocumentType};
use std::path::PathBuf;

#[derive(Args)]
pub struct CheckArgs {
    /// Path to the template file
    pub path: PathBuf,

    /// Extensions to support (comma-separated or repeated). Empty string disables all.
    #[arg(long = "extensions")]
    pub extensions: Option<String>,
}

pub fn execute(args: CheckArgs) -> Result<(), Box<dyn std::error::Error>> {
    let path = &args.path;
    if !path.exists() {
        return Err(format!("'{}' does not exist.", path.display()).into());
    }
    if !path.is_file() {
        return Err(format!("'{}' is not a file.", path.display()).into());
    }

    let content = std::fs::read_to_string(path)?;
    let doc_type = if path.extension().and_then(|e| e.to_str()) == Some("json") {
        DocumentType::Json
    } else {
        DocumentType::Yaml
    };

    let template_value = parse::document_string_to_object(
        &content,
        doc_type,
        &openjd_model::CallerLimits::default(),
    )?;

    let version_str = template_value
        .get("specificationVersion")
        .and_then(|v| v.as_str())
        .ok_or("Missing field 'specificationVersion'")?;

    let exts = crate::common::parse_extensions(&args.extensions)?;
    let supported: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();

    let version = version_str.parse::<openjd_model::TemplateSpecificationVersion>();
    match version {
        Ok(v) if v.is_job_template() => {
            parse::decode_job_template(
                template_value.clone(),
                Some(&supported),
                &openjd_model::CallerLimits::default(),
            )?;
        }
        Ok(v) if v.is_environment_template() => {
            parse::decode_environment_template(template_value.clone(), Some(&supported))?;
        }
        Ok(_) | Err(_) => {
            return Err(format!("Unknown template 'specificationVersion' ({version_str}).").into());
        }
    }

    println!("Template at '{}' passes validation checks.", path.display());
    Ok(())
}
