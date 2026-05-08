// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for path mapping — mirrors Python test_path_mapping.py

use openjd_sessions::path_mapping::{PathFormat, PathMappingRule};

fn posix_rule(src: &str, dst: &str) -> PathMappingRule {
    PathMappingRule {
        source_path_format: PathFormat::Posix,
        source_path: src.to_string(),
        destination_path: dst.to_string(),
    }
}

fn windows_rule(src: &str, dst: &str) -> PathMappingRule {
    PathMappingRule {
        source_path_format: PathFormat::Windows,
        source_path: src.to_string(),
        destination_path: dst.to_string(),
    }
}

// === test_remaps: posix->posix ===

#[test]
fn test_remap_posix_to_posix_sourcepath() {
    let rule = posix_rule("/mnt/shared/", "/newprefix");
    let result = rule.apply_with_format("/mnt/shared", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix".to_string()));
}

#[test]
fn test_remap_posix_to_posix_trailing_slash() {
    let rule = posix_rule("/mnt/shared/", "/newprefix");
    let result = rule.apply_with_format("/mnt/shared/", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/".to_string()));
}

#[test]
fn test_remap_posix_to_posix_1level_file() {
    let rule = posix_rule("/mnt/shared/", "/newprefix");
    let result = rule.apply_with_format("/mnt/shared/file", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/file".to_string()));
}

#[test]
fn test_remap_posix_to_posix_1level_dir() {
    let rule = posix_rule("/mnt/shared/", "/newprefix");
    let result = rule.apply_with_format("/mnt/shared/dir/", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/dir/".to_string()));
}

#[test]
fn test_remap_posix_to_posix_2level_file() {
    let rule = posix_rule("/mnt/shared/", "/newprefix");
    let result = rule.apply_with_format("/mnt/shared/dir/file", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/dir/file".to_string()));
}

#[test]
fn test_remap_posix_to_posix_2level_relative() {
    let rule = posix_rule("/mnt/shared/", "/newprefix");
    let result = rule.apply_with_format("/mnt/shared/dir/../file", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/dir/../file".to_string()));
}

// === test_remaps: posix->windows ===

#[test]
fn test_remap_posix_to_windows_sourcepath() {
    let rule = posix_rule("/mnt/shared/", "c:\\newprefix");
    let result = rule.apply_with_format("/mnt/shared", PathFormat::Windows);
    assert_eq!(result, Some("c:\\newprefix".to_string()));
}

#[test]
fn test_remap_posix_to_windows_trailing_slash() {
    let rule = posix_rule("/mnt/shared/", "c:\\newprefix");
    let result = rule.apply_with_format("/mnt/shared/", PathFormat::Windows);
    assert_eq!(result, Some("c:\\newprefix\\".to_string()));
}

#[test]
fn test_remap_posix_to_windows_1level_file() {
    let rule = posix_rule("/mnt/shared/", "c:\\newprefix");
    let result = rule.apply_with_format("/mnt/shared/file", PathFormat::Windows);
    assert_eq!(result, Some("c:\\newprefix\\file".to_string()));
}

#[test]
fn test_remap_posix_to_windows_2level_file() {
    let rule = posix_rule("/mnt/shared/", "c:\\newprefix");
    let result = rule.apply_with_format("/mnt/shared/dir/file", PathFormat::Windows);
    assert_eq!(result, Some("c:\\newprefix\\dir\\file".to_string()));
}

// === test_remaps: windows->posix ===

#[test]
fn test_remap_windows_to_posix_sourcepath() {
    let rule = windows_rule("c:\\mnt\\shared\\", "/newprefix");
    let result = rule.apply_with_format("c:\\mnt\\shared", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix".to_string()));
}

#[test]
fn test_remap_windows_to_posix_trailing_slash() {
    let rule = windows_rule("c:\\mnt\\shared\\", "/newprefix");
    let result = rule.apply_with_format("c:\\mnt\\shared\\", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/".to_string()));
}

#[test]
fn test_remap_windows_to_posix_1level_file() {
    let rule = windows_rule("c:\\mnt\\shared\\", "/newprefix");
    let result = rule.apply_with_format("c:\\mnt\\shared\\file", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/file".to_string()));
}

#[test]
fn test_remap_windows_to_posix_2level_file() {
    let rule = windows_rule("c:\\mnt\\shared\\", "/newprefix");
    let result = rule.apply_with_format("c:\\mnt\\shared\\dir\\file", PathFormat::Posix);
    assert_eq!(result, Some("/newprefix/dir/file".to_string()));
}

// === test_remaps: windows->windows ===

#[test]
fn test_remap_windows_to_windows_sourcepath() {
    let rule = windows_rule("c:\\mnt\\shared\\", "c:\\newprefix");
    let result = rule.apply_with_format("c:\\mnt\\shared", PathFormat::Windows);
    assert_eq!(result, Some("c:\\newprefix".to_string()));
}

#[test]
fn test_remap_windows_to_windows_1level_file() {
    let rule = windows_rule("c:\\mnt\\shared\\", "c:\\newprefix");
    let result = rule.apply_with_format("c:\\mnt\\shared\\file", PathFormat::Windows);
    assert_eq!(result, Some("c:\\newprefix\\file".to_string()));
}

// === test_remaps: UNC paths ===

#[test]
fn test_remap_unc_from_file() {
    let rule = windows_rule("\\\\128.0.0.1\\share\\assets", "z:\\assets");
    let result = rule.apply_with_format("\\\\128.0.0.1\\share\\assets\\file", PathFormat::Windows);
    assert_eq!(result, Some("z:\\assets\\file".to_string()));
}

#[test]
fn test_remap_unc_to_file() {
    let rule = windows_rule("z:\\assets", "\\\\128.0.0.1\\share\\assets");
    let result = rule.apply_with_format("z:\\assets\\file", PathFormat::Windows);
    assert_eq!(
        result,
        Some("\\\\128.0.0.1\\share\\assets\\file".to_string())
    );
}

// === test_does_not_remap ===

#[test]
fn test_does_not_remap_posix_parent_dir() {
    let rule = posix_rule("/mnt/shared", "c:\\newprefix");
    assert_eq!(rule.apply_with_format("/mnt", PathFormat::Windows), None);
}

#[test]
fn test_does_not_remap_posix_different_dir_too_short() {
    let rule = posix_rule("/mnt/shared", "c:\\newprefix");
    assert_eq!(
        rule.apply_with_format("/mnt/share", PathFormat::Windows),
        None
    );
}

#[test]
fn test_does_not_remap_posix_different_dir_same_prefix() {
    let rule = posix_rule("/mnt/shared", "c:\\newprefix");
    assert_eq!(
        rule.apply_with_format("/mnt/shared2", PathFormat::Windows),
        None
    );
}

#[test]
fn test_does_not_remap_windows_parent_dir() {
    let rule = windows_rule("c:\\mnt\\shared\\", "c:\\newprefix");
    assert_eq!(rule.apply_with_format("c:\\mnt", PathFormat::Windows), None);
}

#[test]
fn test_does_not_remap_windows_different_dir_too_short() {
    let rule = windows_rule("c:\\mnt\\shared\\", "c:\\newprefix");
    assert_eq!(
        rule.apply_with_format("c:\\mnt\\share", PathFormat::Windows),
        None
    );
}

#[test]
fn test_does_not_remap_windows_different_dir_same_prefix() {
    let rule = windows_rule("c:\\mnt\\shared\\", "c:\\newprefix");
    assert_eq!(
        rule.apply_with_format("c:\\mnt\\shared2", PathFormat::Windows),
        None
    );
}

// === test_from_dict_success (via serde) ===

#[test]
fn test_from_dict_windows() {
    let json = r#"{"source_path_format":"WINDOWS","source_path":"C:\\oldprefix","destination_path":"c:\\newprefix"}"#;
    let rule: PathMappingRule = serde_json::from_str(json).unwrap();
    assert_eq!(rule.source_path_format, PathFormat::Windows);
    assert_eq!(rule.source_path, "C:\\oldprefix");
}

#[test]
fn test_from_dict_posix() {
    let json = r#"{"source_path_format":"POSIX","source_path":"/mnt/oldprefix","destination_path":"c:\\newprefix"}"#;
    let rule: PathMappingRule = serde_json::from_str(json).unwrap();
    assert_eq!(rule.source_path_format, PathFormat::Posix);
    assert_eq!(rule.source_path, "/mnt/oldprefix");
}

#[test]
fn test_from_dict_lowercase_windows() {
    let json = r#"{"source_path_format":"windows","source_path":"C:\\oldprefix","destination_path":"c:\\newprefix"}"#;
    let rule: PathMappingRule = serde_json::from_str(json).unwrap();
    assert_eq!(rule.source_path_format, PathFormat::Windows);
}

#[test]
fn test_from_dict_lowercase_posix() {
    let json = r#"{"source_path_format":"posix","source_path":"/mnt/oldprefix","destination_path":"c:\\newprefix"}"#;
    let rule: PathMappingRule = serde_json::from_str(json).unwrap();
    assert_eq!(rule.source_path_format, PathFormat::Posix);
}

// === test_from_dict_failure ===

#[test]
fn test_from_dict_bad_format() {
    let json = r#"{"source_path_format":"WINDOWS10","source_path":"C:\\oldprefix","destination_path":"c:\\newprefix"}"#;
    assert!(serde_json::from_str::<PathMappingRule>(json).is_err());
}

#[test]
fn test_from_dict_missing_format() {
    let json = r#"{"source_path":"/mnt/oldprefix","destination_path":"c:\\newprefix"}"#;
    assert!(serde_json::from_str::<PathMappingRule>(json).is_err());
}

#[test]
fn test_from_dict_missing_source() {
    let json = r#"{"source_path_format":"POSIX","destination_path":"c:\\newprefix"}"#;
    assert!(serde_json::from_str::<PathMappingRule>(json).is_err());
}

#[test]
fn test_from_dict_missing_dest() {
    let json = r#"{"source_path_format":"POSIX","source_path":"/mnt/oldprefix"}"#;
    assert!(serde_json::from_str::<PathMappingRule>(json).is_err());
}

#[test]
fn test_from_dict_extra_field() {
    let json = r#"{"source_path_format":"windows","source_path":"C:\\oldprefix","destination_path":"c:\\newprefix","extra_field":"value"}"#;
    assert!(serde_json::from_str::<PathMappingRule>(json).is_err());
}
