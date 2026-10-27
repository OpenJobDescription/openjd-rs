// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for path mapping materialization.

use openjd_expr::format_string::FormatString;
use openjd_model::job::{Action, StepActions, StepScript};
use openjd_sessions::session::Session;
use openjd_sessions::PathFormat;
use openjd_sessions::PathMappingRule;
use tempfile::TempDir;

fn fs(s: &str) -> FormatString {
    FormatString::new(s).unwrap()
}

#[tokio::test]
async fn test_path_mapping_file_created_with_rules() {
    let tmp = TempDir::new().unwrap();
    let rules = vec![PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/mnt/shared".into(),
        destination_path: "/local/shared".into(),
    }];
    let mut session = Session::new_for_test(tmp.path().to_path_buf())
        .with_path_mapping(rules)
        .unwrap();
    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: fs("cat"),
                args: Some(vec![fs("{{Session.PathMappingRulesFile}}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    assert_eq!(json["version"], "pathmapping-1.0");
    let rules = json["path_mapping_rules"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0]["source_path_format"], "POSIX");
    assert_eq!(rules[0]["source_path"], "/mnt/shared");
    assert_eq!(rules[0]["destination_path"], "/local/shared");
}

#[tokio::test]
async fn test_path_mapping_file_created_empty_when_no_rules() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: fs("cat"),
                args: Some(vec![fs("{{Session.PathMappingRulesFile}}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    // Empty rules should produce empty JSON object
    assert!(json.as_object().unwrap().is_empty() || json.get("path_mapping_rules").is_none());
}

#[tokio::test]
async fn test_has_path_mapping_rules_true() {
    let tmp = TempDir::new().unwrap();
    let rules = vec![PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: "/src".into(),
        destination_path: "/dst".into(),
    }];
    let mut session = Session::new_for_test(tmp.path().to_path_buf())
        .with_path_mapping(rules)
        .unwrap();
    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: fs("sh"),
                args: Some(vec![fs("-c"), fs("echo {{Session.HasPathMappingRules}}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert!(result.stdout.contains("true"));
}

#[tokio::test]
async fn test_has_path_mapping_rules_false() {
    let tmp = TempDir::new().unwrap();
    let mut session = Session::new_for_test(tmp.path().to_path_buf());
    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: fs("sh"),
                args: Some(vec![fs("-c"), fs("echo {{Session.HasPathMappingRules}}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    assert!(result.stdout.contains("false"));
}

#[tokio::test]
async fn test_path_mapping_multiple_rules() {
    let tmp = TempDir::new().unwrap();
    let rule = |format: PathFormat, src: &str, dst: &str| PathMappingRule {
        source_path_format: format,
        source_path: src.into(),
        destination_path: dst.into(),
    };
    // Given in an order that is not the precedence order.
    let rules = vec![
        rule(PathFormat::Windows, "C:\\share", "/local/share"),
        rule(PathFormat::Posix, "/mnt/a", "/first"),
        rule(PathFormat::Posix, "/mnt/a/b", "/local/b"),
        rule(PathFormat::Posix, "/mnt/a/", "/last"),
    ];
    let mut session = Session::new_for_test(tmp.path().to_path_buf())
        .with_path_mapping(rules)
        .unwrap();
    let script = StepScript {
        let_bindings: None,
        actions: StepActions {
            on_run: Action {
                command: fs("cat"),
                args: Some(vec![fs("{{Session.PathMappingRulesFile}}")]),
                timeout: None,
                cancelation: None,
            },
        },
        embedded_files: None,
    };
    let result = session
        .run_task("test_step", &script, None, None, None)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_str(result.stdout.trim()).unwrap();
    let rules = json["path_mapping_rules"].as_array().unwrap();
    let destinations: Vec<&str> = rules
        .iter()
        .map(|r| r["destination_path"].as_str().unwrap())
        .collect();
    // In precedence order: most names first ("/mnt/a/b" has three), then,
    // among the two-name rules "/mnt/a/", "/mnt/a" and "C:\\share" (the
    // drive counts as one name), the one given last first.
    assert_eq!(
        destinations,
        ["/local/b", "/last", "/first", "/local/share"]
    );
    assert_eq!(rules[3]["source_path_format"], "WINDOWS");
    assert_eq!(rules[3]["source_path"], "C:\\share");
}
