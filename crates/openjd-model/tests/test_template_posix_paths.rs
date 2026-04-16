// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests ported from Python test/openjd/model/test_template_posix_paths.py
//!
//! Verifies that TEMPLATE scope expression evaluation uses POSIX path semantics.
//! On Linux this is the default, but these tests document the expected behavior.

use openjd_expr::{ExprValue, ParsedExpression, PathFormat, SymbolTable};

fn eval_posix(expr: &str) -> ExprValue {
    let symtab = SymbolTable::new();
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [&symtab];
    let mut ev = parsed
        .evaluator(&symtabs)
        .with_path_format(PathFormat::Posix);
    ev.evaluate(&parsed.ast).unwrap()
}

fn eval_posix_with(expr: &str, symtab: &SymbolTable) -> ExprValue {
    let parsed = ParsedExpression::new(expr).unwrap();
    let symtabs = [symtab];
    let mut ev = parsed
        .evaluator(&symtabs)
        .with_path_format(PathFormat::Posix);
    ev.evaluate(&parsed.ast).unwrap()
}

// ══════════════════════════════════════════════════════════════
// ExprNode.evaluate uses POSIX paths
// ══════════════════════════════════════════════════════════════

#[test]
fn path_parent_uses_forward_slashes() {
    let result = eval_posix("path('/a/b/c').parent");
    assert_eq!(result.to_display_string(), "/a/b");
}

#[test]
fn path_join_uses_forward_slashes() {
    let result = eval_posix("path('/a/b') / 'c'");
    assert_eq!(result.to_display_string(), "/a/b/c");
}

#[test]
fn path_name_from_posix_path() {
    let result = eval_posix("path('/a/b/file.txt').name");
    assert_eq!(result.to_display_string(), "file.txt");
}

#[test]
fn param_path_parent_uses_forward_slashes() {
    let mut symtab = SymbolTable::new();
    symtab
        .set(
            "Param.Dir",
            ExprValue::Path {
                value: "/projects/shot01/render".to_string(),
                format: PathFormat::Posix,
            },
        )
        .unwrap();
    let result = eval_posix_with("Param.Dir.parent", &symtab);
    assert_eq!(result.to_display_string(), "/projects/shot01");
}

// ══════════════════════════════════════════════════════════════
// evaluate_typed uses POSIX paths
// ══════════════════════════════════════════════════════════════

#[test]
fn path_parent_typed() {
    let result = eval_posix("path('/x/y/z').parent");
    assert_eq!(result.to_display_string(), "/x/y");
}

#[test]
fn path_join_typed() {
    let result = eval_posix("path('/a') / 'b' / 'c'");
    assert_eq!(result.to_display_string(), "/a/b/c");
}

// ══════════════════════════════════════════════════════════════
// create_job uses POSIX paths in TEMPLATE scope
// ══════════════════════════════════════════════════════════════

use openjd_model::{
    create_job, decode_job_template, preprocess_job_parameters, JobParameterInputValues,
};

#[test]
fn job_name_with_path_parent() {
    let v: serde_yaml::Value = serde_yaml::from_str(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{{ path(Param.Dir).parent }}",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {"name": "Dir", "type": "STRING", "default": "/projects/shot01/render"}
        ],
        "steps": [{"name": "Step", "script": {"actions": {"onRun": {"command": "echo hello"}}}}]
    }"#,
    )
    .unwrap();
    let jt = decode_job_template(v, Some(&["EXPR"])).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        ExprValue::String("/projects/shot01/render".into()),
    );
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: std::path::Path::new("/tmp"),
            current_working_dir: std::path::Path::new("/tmp"),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed).unwrap();
    assert!(!job.name.contains('\\'));
    assert_eq!(job.name, "/projects/shot01");
}
