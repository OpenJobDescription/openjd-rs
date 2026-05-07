// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Windows path counterpart to test_template_posix_paths.rs.
//!
//! Verifies that TEMPLATE scope expression evaluation uses Windows path semantics
//! when PathFormat::Windows is specified. These tests run on any host OS.
//!
//! Expression string literals use Python raw strings (r'...') so that backslashes
//! in Windows paths are not interpreted as escape sequences.

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

fn eval_win(expr: &str) -> ExprValue {
    let symtab = SymbolTable::new();
    let parsed = ParsedExpression::new(expr).unwrap();
    parsed
        .with_path_format(PathFormat::Windows)
        .evaluate(&[&symtab])
        .unwrap()
}

fn eval_win_with(expr: &str, symtab: &SymbolTable) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    parsed
        .with_path_format(PathFormat::Windows)
        .evaluate(&[symtab])
        .unwrap()
}

// ══════════════════════════════════════════════════════════════
// ExprNode.evaluate uses Windows paths
// ══════════════════════════════════════════════════════════════

#[test]
fn path_parent_uses_backslashes() {
    let result = eval_win(r"path(r'C:\a\b\c').parent");
    assert_eq!(result.to_display_string(), r"C:\a\b");
}

#[test]
fn path_join_uses_backslashes() {
    let result = eval_win(r"path(r'C:\a\b') / 'c'");
    assert_eq!(result.to_display_string(), r"C:\a\b\c");
}

#[test]
fn path_name_from_windows_path() {
    let result = eval_win(r"path(r'C:\a\b\file.txt').name");
    assert_eq!(result.to_display_string(), "file.txt");
}

#[test]
fn param_path_parent_uses_backslashes() {
    let mut symtab = SymbolTable::new();
    symtab
        .set(
            "Param.Dir",
            ExprValue::new_path(
                r"C:\projects\shot01\render".to_string(),
                PathFormat::Windows,
            ),
        )
        .unwrap();
    let result = eval_win_with("Param.Dir.parent", &symtab);
    assert_eq!(result.to_display_string(), r"C:\projects\shot01");
}

// ══════════════════════════════════════════════════════════════
// evaluate_typed uses Windows paths
// ══════════════════════════════════════════════════════════════

#[test]
fn path_parent_typed() {
    let result = eval_win(r"path(r'C:\x\y\z').parent");
    assert_eq!(result.to_display_string(), r"C:\x\y");
}

#[test]
fn path_join_typed() {
    let result = eval_win(r"path(r'C:\a') / 'b' / 'c'");
    assert_eq!(result.to_display_string(), r"C:\a\b\c");
}

// ══════════════════════════════════════════════════════════════
// create_job uses Windows paths in TEMPLATE scope
// ══════════════════════════════════════════════════════════════

use openjd_model::CallerLimits;
use openjd_model::{
    create_job, decode_job_template, preprocess_job_parameters, JobParameterInputValues,
};

#[test]
fn create_job_with_windows_path_parameter() {
    // Verify the full create_job pipeline works with Windows path format.
    // Job name resolution is hardcoded to POSIX, so we test that the pipeline
    // succeeds and the STRING parameter value is preserved correctly.
    let v: serde_json::Value = serde_saphyr::from_str(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{{ Param.Dir }}",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {"name": "Dir", "type": "STRING", "default": "C:\\projects\\shot01\\render"}
        ],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo hello"}}}}]
    }"#,
    )
    .unwrap();
    let jt = decode_job_template(v, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        ExprValue::String(r"C:\projects\shot01\render".into()),
    );
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: r"C:\tmp",
            current_working_dir: r"C:\tmp",
            allow_template_dir_walk_up: false,
            path_format: PathFormat::Windows,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    assert_eq!(job.name, r"C:\projects\shot01\render");
}
