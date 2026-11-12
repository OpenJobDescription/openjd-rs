// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! `openjd build` command — print the job a template builds into.

use std::path::{Path, PathBuf};

use clap::Args;
use openjd_model::job::template_diff::{JobDocument, TemplateInput, TemplateSource};

const EXAMPLES: &str = "\
Examples:
  openjd build render.yaml -p Frames=1-100 > old.json
  openjd build render.yaml -p file://params.json --output yaml";

#[derive(Args)]
#[command(after_help = EXAMPLES)]
pub struct BuildArgs {
    /// Path to the job template file
    pub path: PathBuf,

    /// Job parameters (Key=Value, file://path, or inline JSON)
    #[arg(short = 'p', long = "job-param", alias = "parameter")]
    pub parameters: Vec<String>,

    /// Environment template files
    #[arg(long = "environment", alias = "env")]
    pub environments: Vec<PathBuf>,

    /// Extensions to support (comma-separated). Empty string disables all
    #[arg(long = "extensions")]
    pub extensions: Option<String>,

    /// How to format the job
    #[arg(long = "output", value_parser = ["json", "yaml"], default_value = "json")]
    pub output: String,
}

/// Printed to stderr when the command starts; stdout stays the job document.
const UNSTABLE_WARNING: &str =
    "warning: 'openjd build' is unstable; its flags and output may change.";

pub fn execute(args: BuildArgs) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("{UNSTABLE_WARNING}");
    let exts = crate::common::parse_extensions(&args.extensions)?;
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    crate::common::read_input_file(&args.path)?;
    let mut input = TemplateInput::new(TemplateSource::Path(args.path.clone()))
        .extensions(&exts)
        .limits(crate::common::caller_limits())
        .params(crate::run::parse_cli_parameters(&args.parameters)?);
    for env in &args.environments {
        crate::common::read_input_file(Path::new(env))?;
        input = input.environment(TemplateSource::Path(env.clone()));
    }
    let job = input.build()?;
    // Serialized directly, not through `serde_json::Value`, which would sort
    // task parameter definitions by name and so change task order.
    if args.output == "yaml" {
        print!("{}", serde_saphyr::to_string(&JobDocument(&job))?);
    } else {
        println!("{}", serde_json::to_string_pretty(&JobDocument(&job))?);
    }
    Ok(())
}
