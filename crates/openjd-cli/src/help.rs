// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Context-aware help for `openjd-rs run <template> --help`.

use openjd_model::parse::{self, DocumentType};
use openjd_model::{JobParameterDefinition, JobTemplate};
use std::path::Path;

/// Check if the CLI args are `run <path> -h/--help` and if so, print
/// context-aware help and exit. Returns true if handled.
pub fn try_context_aware_help(args: &[String]) -> bool {
    // Need at least: binary, "run", <path>, and -h/--help somewhere
    if args.len() < 3 {
        return false;
    }
    if args[1] != "run" {
        return false;
    }
    if !args.iter().any(|a| a == "-h" || a == "--help") {
        return false;
    }

    // Find the template path — first positional arg after "run" that doesn't start with -
    let template_path = args[2..]
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str());

    let Some(path_str) = template_path else {
        // No template path, let clap show standard help
        return false;
    };

    let path = Path::new(path_str);

    // Find --extensions value if present
    let extensions = args
        .windows(2)
        .find(|w| w[0] == "--extensions")
        .map(|w| w[1].as_str());

    match generate_template_help(path, extensions) {
        Ok(help) => {
            print!("{help}");
            std::process::exit(0);
        }
        Err(e) => {
            eprint!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn generate_template_help(
    path: &Path,
    extensions_arg: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Err(format!("'{}' not found or does not exist.", path.display()).into());
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

    let default_exts = vec![
        "TASK_CHUNKING",
        "REDACTED_ENV_VARS",
        "FEATURE_BUNDLE_1",
        "EXPR",
    ];
    let supported_exts: Vec<&str> = match extensions_arg {
        Some(s) if !s.is_empty() => s.split(',').map(|e| e.trim()).collect(),
        Some(_) => vec![],
        None => default_exts,
    };

    let template = parse::decode_job_template(
        template_value,
        Some(&supported_exts),
        &openjd_model::CallerLimits::default(),
    )
    .map_err(|e| format!("Invalid job template: {e}"))?;

    Ok(format_help(&template, path))
}

pub fn format_help(template: &JobTemplate, path: &Path) -> String {
    let mut out = String::new();

    // Usage line
    out.push_str(&format!(
        "usage: openjd-rs run {} [arguments]\n\n",
        path.display()
    ));

    // Job name
    out.push_str(&format!("Job: {}\n", template.name()));

    // Description
    if let Some(desc) = template.description() {
        out.push_str(&format!("{}\n", desc));
    }
    out.push('\n');

    // Parameters
    let params = template.parameter_definitions_list();
    if !params.is_empty() {
        out.push_str("Job Parameters (-p/--job-param PARAM_NAME=VALUE):\n");
        for param in params {
            out.push_str("  ");
            out.push_str(&format_parameter(param));
            out.push('\n');
        }
        out.push('\n');
    }

    // Standard options
    out.push_str("Standard Options:\n");
    out.push_str(
        "  -p, --job-param <KEY=VALUE>  Job parameters (Key=Value, file://path, or inline JSON)\n",
    );
    out.push_str("  --environment <PATH>       Environment template files\n");
    out.push_str("  --verbose                  Enable verbose logging\n");
    out.push_str(
        "  -h, --help                 Print help (leave out template to list all options)\n",
    );

    out
}

fn format_parameter(param: &JobParameterDefinition) -> String {
    let name = param.name();
    let type_name = param.type_name();

    let mut line = format!("{name} ({type_name})");

    // Default or required
    let default = param.default_value();
    let is_string_like = matches!(type_name, "STRING" | "PATH");
    match &default {
        Some(val) if is_string_like && val.contains('\n') => {
            line.push_str(" [default: see below]");
        }
        Some(val) if is_string_like => {
            line.push_str(&format!(" [default: '{val}']"));
        }
        Some(val) => {
            line.push_str(&format!(" [default: {val}]"));
        }
        None => {
            line.push_str(" [required]");
        }
    }

    // Constraints
    let mut constraints = Vec::new();

    // Numeric range
    let min_i = param.min_value_i64();
    let max_i = param.max_value_i64();
    let min_f = param.min_value_f64();
    let max_f = param.max_value_f64();

    if type_name == "FLOAT" {
        match (min_f, max_f) {
            (Some(lo), Some(hi)) => constraints.push(format!("range: {lo} to {hi}")),
            (Some(lo), None) => constraints.push(format!("minimum: {lo}")),
            (None, Some(hi)) => constraints.push(format!("maximum: {hi}")),
            _ => {}
        }
    } else if type_name == "INT" {
        match (min_i, max_i) {
            (Some(lo), Some(hi)) => constraints.push(format!("range: {lo} to {hi}")),
            (Some(lo), None) => constraints.push(format!("minimum: {lo}")),
            (None, Some(hi)) => constraints.push(format!("maximum: {hi}")),
            _ => {}
        }
    }

    // String length
    match (param.min_length(), param.max_length()) {
        (Some(lo), Some(hi)) => constraints.push(format!("length: {lo} to {hi} characters")),
        (Some(lo), None) => constraints.push(format!("minimum length: {lo} characters")),
        (None, Some(hi)) => constraints.push(format!("maximum length: {hi} characters")),
        _ => {}
    }

    // Allowed values
    if let Some(vals) = param.allowed_values_strings() {
        if is_string_like {
            let formatted: Vec<String> = vals.iter().map(|v| format!("'{v}'")).collect();
            constraints.push(format!("allowed: {}", formatted.join(", ")));
        } else {
            constraints.push(format!("allowed: {}", vals.join(", ")));
        }
    }

    if !constraints.is_empty() {
        line.push_str(&format!(" ({})", constraints.join(", ")));
    }

    // Description
    if let Some(d) = param.description() {
        line.push_str(&format!("\n    {d}"));
    }

    // Multi-line default
    if let Some(val) = &default {
        if val.contains('\n') {
            line.push_str("\n    Default value:");
            for dl in val.lines() {
                line.push_str(&format!("\n      {dl}"));
            }
        }
    }

    line
}
