// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! openjd-cli argument parsers (`--job-param`, `--task-param`, `--tasks`,
//! `--path-mapping-rules`, `--extensions`) on arbitrary text. Each line of
//! the input is one argument; `file://` arguments are skipped so the target
//! never touches the filesystem.
//!
//! Oracles: `Name=Value` arguments parse to exactly that pair; a number in
//! an inline JSON argument is a float for a FLOAT parameter and text that
//! reads back as the same number otherwise; a parsed task-parameter set has
//! one entry per argument; parsed extensions are all supported names.

use crate::cli_params::{
    load_path_mapping_rules, parse_cli_parameters, parse_task_params, parse_tasks_arg,
};
use crate::common::parse_extensions;
use crate::oracle;
use crate::targets::text_and_control;
use arbitrary::Unstructured;
use openjd_expr::ExprValue;
use openjd_model::ModelExtension;
use std::collections::HashSet;

pub fn args(data: &[u8]) {
    let Some((text, ctl)) = text_and_control(data) else {
        return;
    };
    if crate::known::skip_deep_interpolations(text) {
        return;
    }
    if text.contains("file://") {
        return;
    }
    let lines: Vec<String> = text.lines().map(str::to_string).collect();

    // The FLOAT parameters the templates would define: any of the names
    // the arguments give, as the control bytes pick.
    let mut u = Unstructured::new(ctl);
    let mut float_params = HashSet::new();
    for l in &lines {
        let l = l.trim();
        let names: Vec<String> = match serde_json::from_str::<serde_json::Value>(l) {
            Ok(serde_json::Value::Object(o)) if l.starts_with('{') => o.keys().cloned().collect(),
            _ => l
                .split_once('=')
                .map(|(k, _)| k.to_string())
                .into_iter()
                .collect(),
        };
        for name in names {
            if u.arbitrary::<bool>().unwrap_or(false) {
                float_params.insert(name);
            }
        }
    }

    if let [only] = lines.as_slice() {
        json_numbers(only.trim(), &float_params);
    }

    if let Ok(params) = parse_cli_parameters(&lines, &float_params) {
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
        // Each argument is `NAME=VALUE` (trimmed, split at the first `=`).
        for arg in &lines {
            if let Some((k, v)) = arg.trim().split_once('=') {
                oracle!(
                    params.get(k).map(String::as_str) == Some(v),
                    "task param {arg:?} parsed to {:?}",
                    params.get(k)
                );
            }
        }
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

/// A single inline JSON `--job-param` argument: a number given to a FLOAT
/// parameter is that float; given to any other parameter it is text, an
/// integer's JSON spelling or a float that reads back as the same `f64`.
fn json_numbers(arg: &str, float_params: &HashSet<String>) {
    if !(arg.starts_with('{') && arg.ends_with('}')) {
        return;
    }
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(arg) else {
        return;
    };
    let Ok(params) = parse_cli_parameters(&[arg.to_string()], float_params) else {
        oracle!(
            false,
            "--job-param {arg:?} is a JSON object but was rejected"
        );
        return;
    };
    for (k, v) in &obj {
        let serde_json::Value::Number(n) = v else {
            continue;
        };
        let Some(f) = n.as_f64() else {
            continue;
        };
        let got = params.get(k);
        let ok = match got {
            Some(ExprValue::Float(x)) => float_params.contains(k) && x.value() == f,
            Some(ExprValue::String(s)) if !float_params.contains(k) => {
                if n.is_f64() {
                    s.parse::<f64>()
                        .is_ok_and(|r| r == f && r.is_sign_negative() == f.is_sign_negative())
                } else {
                    *s == n.to_string()
                }
            }
            _ => false,
        };
        oracle!(
            ok,
            "--job-param {arg:?}: {k:?} = {n} parsed to {got:?} (FLOAT: {})",
            float_params.contains(k)
        );
    }
}
