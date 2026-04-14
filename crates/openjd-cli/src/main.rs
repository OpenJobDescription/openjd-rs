// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! OpenJD CLI — validate, summarize, and run job templates.

mod check;
mod common;
mod help;
pub mod run;
mod summary;

use clap::{Parser, Subcommand};
use log::{LevelFilter, Log, Metadata, Record};
use std::sync::OnceLock;
use std::time::Instant;

/// Global session start time and timestamp format for the logger.
static SESSION_START: OnceLock<Instant> = OnceLock::new();
static TIMESTAMP_FORMAT: OnceLock<String> = OnceLock::new();

fn format_log_timestamp() -> String {
    let fmt = TIMESTAMP_FORMAT
        .get()
        .map(|s| s.as_str())
        .unwrap_or("relative");
    match fmt {
        "local" => chrono::Local::now()
            .format("%Y-%m-%dT%H:%M:%S%.3f")
            .to_string(),
        "utc" => chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        _ => {
            let start = SESSION_START.get().copied().unwrap_or_else(Instant::now);
            let d = start.elapsed();
            let h = d.as_secs() / 3600;
            let m = (d.as_secs() % 3600) / 60;
            let s = d.as_secs() % 60;
            let ms = d.subsec_millis();
            format!("{h}:{m:02}:{s:02}.{ms:03}")
        }
    }
}

/// Logger that prints subprocess COMMAND_OUTPUT lines to stdout with timestamps.
struct SessionLogger;

impl Log for SessionLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        struct Visitor {
            bits: Option<u64>,
        }
        impl<'kvs> log::kv::VisitSource<'kvs> for Visitor {
            fn visit_pair(
                &mut self,
                key: log::kv::Key<'kvs>,
                value: log::kv::Value<'kvs>,
            ) -> Result<(), log::kv::Error> {
                if key.as_str() == "openjd_log_content" {
                    self.bits = value.to_u64();
                }
                Ok(())
            }
        }
        let mut v = Visitor { bits: None };
        let _ = record.key_values().visit(&mut v);
        if let Some(bits) = v.bits {
            if bits & 8 != 0 {
                let ts = format_log_timestamp();
                println!("{ts}\t{}", record.args());
            }
        }
    }

    fn flush(&self) {}
}

#[derive(Parser)]
#[command(name = "openjd-rs", about = "Open Job Description CLI (Rust)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check a template for validation errors
    Check(check::CheckArgs),
    /// Print summary information about a Job Template
    Summary(summary::SummaryArgs),
    /// Run a job template locally
    Run(run::RunArgs),
}

#[tokio::main]
async fn main() {
    log::set_boxed_logger(Box::new(SessionLogger)).expect("failed to set logger");
    log::set_max_level(LevelFilter::Info);

    // Intercept `run <path> -h/--help` for context-aware help
    let args: Vec<String> = std::env::args().collect();
    if help::try_context_aware_help(&args) {
        return;
    }

    // Rewrite -tp to --task-param for Python CLI compatibility
    // (clap doesn't support multi-char short flags)
    let rewritten: Vec<String> = args
        .iter()
        .map(|a| {
            if a == "-tp" {
                "--task-param".to_string()
            } else {
                a.clone()
            }
        })
        .collect();
    let cli = Cli::parse_from(rewritten);
    let result = match cli.command {
        Commands::Check(args) => check::execute(args),
        Commands::Summary(args) => summary::execute(args),
        Commands::Run(args) => run::execute(args).await,
    };
    if let Err(e) = result {
        eprintln!("ERROR: {e}");
        std::process::exit(1);
    }
}
