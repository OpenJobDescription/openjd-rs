// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-cli argument parsers (`--job-param`, `--task-param`, `--tasks`,
//! `--path-mapping-rules`, `--extensions`) on arbitrary text. Each line of
//! the input is one argument; `file://` arguments are skipped so the target
//! never touches the filesystem.
//!
//! Oracles: `Name=Value` arguments parse to exactly that pair; a parsed
//! task-parameter set has one entry per argument; parsed extensions are all
//! supported names.

use crate::cli_params::{
    load_path_mapping_rules, parse_cli_parameters, parse_task_params, parse_tasks_arg,
};
use crate::common::parse_extensions;
use crate::oracle;
use crate::targets::text_and_control;
use openjd_expr::ExprValue;
use openjd_model::ModelExtension;

pub fn args(data: &[u8]) {
    let Some((text, _)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_interpolations(text) {
        return;
    }
    if text.contains("file://") {
        return;
    }
    let lines: Vec<String> = text.lines().map(str::to_string).collect();

    if let Ok(params) = parse_cli_parameters(&lines) {
        for arg in &lines {
            let arg = arg.trim();
            if arg.starts_with('{') {
                continue;
            }
            if let Some((k, v)) = arg.split_once('=') {
                // A later argument may override an earlier one.
                let last = lines.iter().rev().find_map(|l| {
                    let l = l.trim();
                    if l.starts_with('{') {
                        None
                    } else {
                        l.split_once('=')
                            .filter(|(lk, _)| *lk == k)
                            .map(|(_, lv)| lv)
                    }
                });
                if lines.iter().any(|l| l.trim().starts_with('{')) {
                    continue;
                }
                oracle!(
                    params.get(k).is_some_and(
                        |p| matches!(p, ExprValue::String(s) if Some(s.as_str()) == last)
                    ),
                    "--job-param {arg:?} parsed to {:?} (expected {v:?})",
                    params.get(k)
                );
            }
        }
    }

    if let Ok(params) = parse_task_params(&lines) {
        oracle!(
            params.len() == lines.len(),
            "{} task params parsed from {} arguments",
            params.len(),
            lines.len()
        );
    }

    let _ = parse_tasks_arg(text);
    let _ = load_path_mapping_rules(&Some(text.to_string()));

    if let Ok(exts) = parse_extensions(&Some(text.to_string())) {
        for e in &exts {
            oracle!(
                ModelExtension::ALL.iter().any(|m| m.as_str() == e),
                "parse_extensions accepted unknown extension {e:?}"
            );
        }
    }
}
