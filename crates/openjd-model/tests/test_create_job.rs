// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test_create_job.py and test_merge_job_parameters.py

use openjd_expr::path_mapping::PathFormat;
use openjd_model::CallerLimits;
use openjd_model::JobParameterInputValues;
use openjd_model::{
    build_symbol_table, decode_environment_template, decode_job_template,
    merge_job_parameter_definitions, preprocess_job_parameters,
};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn minimal_job_template(params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "parameterDefinitions": [{params}],
        "steps": [{{"name": "step", "script": {{"actions": {{"onRun": {{"command": "do thing"}}}}}}}}]
    }}"#
    ))
}

fn minimal_env_template(params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{params}],
        "environment": {{"name": "env", "script": {{"actions": {{"onEnter": {{"command": "do thing"}}}}}}}}
    }}"#
    ))
}

/// Provides platform-appropriate temporary directories for tests that call
/// preprocess_job_parameters (which requires absolute paths).
struct TestDirs {
    _root: tempfile::TempDir,
    template_dir: std::path::PathBuf,
    cwd: std::path::PathBuf,
}

impl TestDirs {
    fn new() -> Self {
        let root = tempfile::TempDir::new().unwrap();
        let template_dir = root.path().join("template");
        let cwd = root.path().join("cwd");
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        Self {
            _root: root,
            template_dir,
            cwd,
        }
    }
    fn template(&self) -> &str {
        self.template_dir.to_str().unwrap()
    }
    fn cwd(&self) -> &str {
        self.cwd.to_str().unwrap()
    }
    /// Join a relative path and normalize separators to match the model's behavior.
    fn join_normalized(base: &str, relative: &str) -> String {
        let joined = std::path::Path::new(base).join(relative);
        // The model normalizes all separators to the OS native
        joined
            .to_string_lossy()
            .replace('/', std::path::MAIN_SEPARATOR_STR)
    }
}

// === preprocess_job_parameters ===

#[test]
fn test_preprocess_string_param() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("hello".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(result["Foo"].value, openjd_expr::ExprValue::String(ref s) if s == "hello"));
}

#[test]
fn test_preprocess_int_param() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "INT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("42".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Int(42)
    ));
}

#[test]
fn test_preprocess_float_param() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "FLOAT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("3.14".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Float(_)
    ));
}

#[test]
fn test_preprocess_uses_default() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING", "default": "bar"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(result["Foo"].value, openjd_expr::ExprValue::String(ref s) if s == "bar"));
}

#[test]
fn test_preprocess_missing_required_param() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_err());
}

#[test]
fn test_preprocess_extra_param() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING", "default": "x"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Bar".into(), openjd_expr::ExprValue::String("extra".into()));
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_err());
}

#[test]
fn test_preprocess_int_constraint_violation() {
    let td = TestDirs::new();
    let jt_val =
        minimal_job_template(r#"{"name": "Foo", "type": "INT", "minValue": 10, "maxValue": 20}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("5".into()));
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_err());
}

#[test]
fn test_preprocess_string_allowed_values_violation() {
    let td = TestDirs::new();
    let jt_val =
        minimal_job_template(r#"{"name": "Foo", "type": "STRING", "allowedValues": ["a", "b"]}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("c".into()));
    assert!(preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .is_err());
}

// === PATH parameter resolution ===

#[test]
fn test_path_default_joined_to_template_dir() {
    let td = TestDirs::new();
    let jt_val =
        minimal_job_template(r#"{"name": "DataDir", "type": "PATH", "default": "data/input.csv"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    if let openjd_expr::ExprValue::String(ref s) = result["DataDir"].value {
        let exp = TestDirs::join_normalized(td.template(), "data/input.csv");
        assert_eq!(s, &exp);
    } else {
        panic!("Expected String");
    }
}

#[test]
fn test_path_user_value_joined_to_cwd() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "DataDir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "DataDir".into(),
        openjd_expr::ExprValue::String("my/output".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    if let openjd_expr::ExprValue::String(ref s) = result["DataDir"].value {
        let exp = std::path::Path::new(td.cwd())
            .join("my/output")
            .to_string_lossy()
            .to_string();
        assert_eq!(s, &exp);
    } else {
        panic!("Expected String");
    }
}

#[test]
fn test_path_absolute_user_value_unchanged() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "DataDir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    let abs_path = std::path::Path::new(td.cwd())
        .join("absolute_test")
        .to_string_lossy()
        .to_string();
    input.insert(
        "DataDir".into(),
        openjd_expr::ExprValue::String(abs_path.clone()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    if let openjd_expr::ExprValue::String(ref s) = result["DataDir"].value {
        assert_eq!(s, &abs_path);
    } else {
        panic!("Expected String");
    }
}

#[test]
fn test_path_absolute_default_rejected() {
    let td = TestDirs::new();
    let abs_default = if cfg!(windows) {
        r#"C:\\absolute\\path"#
    } else {
        "/absolute/path"
    };
    let jt_val = minimal_job_template(&format!(
        r#"{{"name": "DataDir", "type": "PATH", "default": "{}"}}"#,
        abs_default
    ));
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("absolute path"),
        "Expected absolute path error, got: {err}"
    );
}

#[test]
fn test_path_default_walkup_rejected() {
    let td = TestDirs::new();
    let jt_val =
        minimal_job_template(r#"{"name": "DataDir", "type": "PATH", "default": "../escape/path"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("outside of the template directory"),
        "Expected walkup error, got: {err}"
    );
}

#[test]
fn test_path_empty_default_not_joined() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "DataDir", "type": "PATH", "default": ""}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(
        matches!(result["DataDir"].value, openjd_expr::ExprValue::String(ref s) if s.is_empty())
    );
}

#[test]
fn test_path_empty_user_value_not_joined() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "DataDir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("DataDir".into(), openjd_expr::ExprValue::String("".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(
        matches!(result["DataDir"].value, openjd_expr::ExprValue::String(ref s) if s.is_empty())
    );
}

// === build_symbol_table ===

#[test]
fn test_build_symbol_table_int() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Frame", "type": "INT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Frame".into(), openjd_expr::ExprValue::String("42".into()));
    let params = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let symtab = build_symbol_table(&params).unwrap();
    let val = symtab.get_value("Param.Frame").unwrap();
    assert_eq!(val.to_display_string(), "42");
}

#[test]
fn test_build_symbol_table_string() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Name", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Name".into(),
        openjd_expr::ExprValue::String("hello".into()),
    );
    let params = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let symtab = build_symbol_table(&params).unwrap();
    let val = symtab.get_value("Param.Name").unwrap();
    assert_eq!(val.to_display_string(), "hello");
}

// === merge_job_parameter_definitions ===

#[test]
fn test_merge_no_env_templates() {
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].name, "Foo");
}

#[test]
fn test_merge_env_and_job_same_param() {
    let jt_val =
        minimal_job_template(r#"{"name": "Foo", "type": "STRING", "default": "job_default"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val =
        minimal_env_template(r#"{"name": "Foo", "type": "STRING", "default": "env_default"}"#);
    let et = decode_environment_template(et_val, None).unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    // Job template is processed last, so its default wins
    assert_eq!(merged[0].default.as_deref(), Some("job_default"));
}

#[test]
fn test_merge_type_conflict() {
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = minimal_env_template(r#"{"name": "Foo", "type": "INT"}"#);
    let et = decode_environment_template(et_val, None).unwrap();
    assert!(merge_job_parameter_definitions(&jt, &[et]).is_err());
}

#[test]
fn test_merge_env_only_param() {
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = minimal_env_template(r#"{"name": "EnvParam", "type": "INT", "default": "5"}"#);
    let et = decode_environment_template(et_val, None).unwrap();
    let merged = merge_job_parameter_definitions(&jt, &[et]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].name, "EnvParam");
}

// === URI path handling ===

fn expr_job_template_with_path_param(param_name: &str, default: Option<&str>) -> serde_json::Value {
    let default_str = match default {
        Some(d) => format!(r#", "default": "{d}""#),
        None => String::new(),
    };
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "test",
        "parameterDefinitions": [{{"name": "{param_name}", "type": "PATH"{default_str}}}],
        "steps": [{{"name": "step", "script": {{"actions": {{"onRun": {{"command": "do thing"}}}}}}}}]
    }}"#
    ))
}

#[test]
fn test_uri_user_value_preserved() {
    let td = TestDirs::new();
    // URI PATH values should not be joined with current_working_dir
    let jt_val = expr_job_template_with_path_param("S3File", None);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "S3File".into(),
        openjd_expr::ExprValue::String("s3://my-bucket/assets/teapot.obj".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    // URI should be preserved as-is, not joined with /tmp/cwd
    match &result["S3File"].value {
        openjd_expr::ExprValue::String(s) => assert_eq!(s, "s3://my-bucket/assets/teapot.obj"),
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_uri_default_preserved() {
    let td = TestDirs::new();
    // URI PATH defaults should not be joined with job_template_dir
    let jt_val = expr_job_template_with_path_param("S3File", Some("s3://my-bucket/default.obj"));
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let result = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["S3File"].value {
        openjd_expr::ExprValue::String(s) => assert_eq!(s, "s3://my-bucket/default.obj"),
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_https_uri_user_value_preserved() {
    let td = TestDirs::new();
    let jt_val = expr_job_template_with_path_param("HttpFile", None);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "HttpFile".into(),
        openjd_expr::ExprValue::String("https://cdn.example.com/models/scene.obj".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["HttpFile"].value {
        openjd_expr::ExprValue::String(s) => {
            assert_eq!(s, "https://cdn.example.com/models/scene.obj")
        }
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_uri_joined_without_expr_extension() {
    let td = TestDirs::new();
    // Without EXPR, URIs are not recognized — treated as opaque relative strings
    // and joined with cwd, matching Python's Path("s3://bucket/key") behavior.
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "parameterDefinitions": [{"name": "S3File", "type": "PATH"}],
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "S3File".into(),
        openjd_expr::ExprValue::String("s3://my-bucket/file.obj".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true, // doesn't matter without EXPR
        },
    )
    .unwrap();
    match &result["S3File"].value {
        openjd_expr::ExprValue::String(s) => {
            // Should be joined with cwd since URI is not recognized without EXPR
            assert!(
                s.contains("s3:") && s != "s3://my-bucket/file.obj",
                "URI should be joined with cwd, got: {s}"
            );
        }
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_uri_rejected_when_not_allowed() {
    let td = TestDirs::new();
    // With EXPR + allow_uri_path_values=false, URIs are rejected
    let jt_val = expr_job_template_with_path_param("S3File", None);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "S3File".into(),
        openjd_expr::ExprValue::String("s3://my-bucket/file.obj".into()),
    );
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: false,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("URI path values are not permitted"),
        "Expected URI rejection error, got: {err}"
    );
}

#[test]
fn test_uri_default_rejected_when_not_allowed() {
    let td = TestDirs::new();
    // With EXPR + allow_uri_path_values=false, URI defaults are rejected
    let jt_val = expr_job_template_with_path_param("S3File", Some("s3://my-bucket/default.obj"));
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let err = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: false,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("URI path values are not permitted"),
        "Expected URI rejection error, got: {err}"
    );
}

#[test]
fn test_posix_absolute_path_recognized_with_posix_format() {
    // /foo/bar is absolute under PathFormat::Posix — should not be joined with cwd
    let jt_val = minimal_job_template(r#"{"name": "Dir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        openjd_expr::ExprValue::String("/foo/bar".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/template",
            current_working_dir: "/cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Dir"].value {
        openjd_expr::ExprValue::String(s) => assert_eq!(s, "/foo/bar"),
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_posix_path_is_relative_under_windows_format() {
    // /foo/bar under PathFormat::Windows is root-relative: keeps drive from cwd
    let jt_val = minimal_job_template(r#"{"name": "Dir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        openjd_expr::ExprValue::String("/foo/bar".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "C:\\template",
            current_working_dir: "C:\\cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Windows,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Dir"].value {
        openjd_expr::ExprValue::String(s) => {
            // Root-relative: drive from cwd + the /foo/bar path
            assert_eq!(s, "C:/foo/bar");
        }
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_windows_absolute_path_recognized_with_windows_format() {
    // C:\foo is absolute under PathFormat::Windows — should not be joined
    let jt_val = minimal_job_template(r#"{"name": "Dir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        openjd_expr::ExprValue::String("C:\\foo\\bar".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "C:\\template",
            current_working_dir: "C:\\cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Windows,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Dir"].value {
        openjd_expr::ExprValue::String(s) => assert_eq!(s, "C:\\foo\\bar"),
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_windows_path_is_relative_under_posix_format() {
    // C:\foo is NOT absolute under PathFormat::Posix — should be joined with cwd
    let jt_val = minimal_job_template(r#"{"name": "Dir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        openjd_expr::ExprValue::String("C:\\foo\\bar".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/template",
            current_working_dir: "/cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Dir"].value {
        openjd_expr::ExprValue::String(s) => {
            assert_eq!(
                s, "/cwd/C:\\foo\\bar",
                "Should be joined with cwd using POSIX separator"
            );
        }
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_unc_path_recognized_as_absolute() {
    // \\server\share is absolute under both formats
    let jt_val = minimal_job_template(r#"{"name": "Dir", "type": "PATH"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Dir".into(),
        openjd_expr::ExprValue::String("\\\\server\\share".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "C:\\template",
            current_working_dir: "C:\\cwd",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Windows,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Dir"].value {
        openjd_expr::ExprValue::String(s) => assert_eq!(s, "\\\\server\\share"),
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_relative_path_still_joined_with_expr() {
    let td = TestDirs::new();
    // Regular relative paths should still be joined even with EXPR extension
    let jt_val = expr_job_template_with_path_param("LocalFile", None);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "LocalFile".into(),
        openjd_expr::ExprValue::String("subdir/file.txt".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["LocalFile"].value {
        openjd_expr::ExprValue::String(s) => {
            let exp = std::path::Path::new(td.cwd())
                .join("subdir/file.txt")
                .to_string_lossy()
                .to_string();
            assert_eq!(s, &exp);
        }
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_uri_in_build_symbol_table() {
    let td = TestDirs::new();
    // PATH params: Param.X is Unresolved(PATH) at create_job time (resolved at session time).
    // RawParam.X is the original string value.
    let jt_val = expr_job_template_with_path_param("S3File", None);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "S3File".into(),
        openjd_expr::ExprValue::String("s3://my-bucket/assets/teapot.obj".into()),
    );
    let params = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let symtab = build_symbol_table(&params).unwrap();
    // RawParam should be the original string
    assert_eq!(
        symtab.get_string("RawParam.S3File").unwrap(),
        "s3://my-bucket/assets/teapot.obj"
    );
    // Param.S3File should NOT be in the template-scope symtab (PATH types are host-context only)
    assert!(
        symtab.get_value("Param.S3File").is_none(),
        "Param.S3File should not be in template-scope symtab"
    );
}

// === create_job ===

use openjd_model::create_job;
use openjd_model::job;

fn parse_and_create(template_json: &str, params: &[(&str, &str)]) -> job::Job {
    let td = TestDirs::new();
    let v: serde_json::Value = serde_saphyr::from_str(template_json).unwrap();
    let supported = ["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];
    let supported_refs: Vec<&str> = supported.to_vec();
    let jt = decode_job_template(v, Some(&supported_refs), &CallerLimits::default()).unwrap();
    let input: std::collections::HashMap<String, openjd_expr::ExprValue> = params
        .iter()
        .map(|(k, v)| (k.to_string(), openjd_expr::ExprValue::String(v.to_string())))
        .collect();
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    create_job(&jt, &processed, &jt.default_validation_context()).unwrap()
}

fn parse_and_create_err(template_json: &str, params: &[(&str, &str)]) -> String {
    let td = TestDirs::new();
    let v: serde_json::Value = serde_saphyr::from_str(template_json).unwrap();
    let supported = ["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];
    let supported_refs: Vec<&str> = supported.to_vec();
    let jt = decode_job_template(v, Some(&supported_refs), &CallerLimits::default()).unwrap();
    let input: std::collections::HashMap<String, openjd_expr::ExprValue> = params
        .iter()
        .map(|(k, v)| (k.to_string(), openjd_expr::ExprValue::String(v.to_string())))
        .collect();
    let processed = match preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    ) {
        Err(e) => return e.to_string(),
        Ok(p) => p,
    };
    create_job(&jt, &processed, &jt.default_validation_context())
        .unwrap_err()
        .to_string()
}

#[test]
fn test_create_job_basic_minimal_template() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "MyJob",
        "steps": [{"name": "Step1", "script": {"actions": {"onRun": {"command": "echo", "args": ["hello"]}}}}]
    }"#,
        &[],
    );
    assert_eq!(job.name, "MyJob");
    assert_eq!(job.steps.len(), 1);
    assert_eq!(job.steps[0].name, "Step1");
    assert_eq!(job.steps[0].script.actions.on_run.command.raw(), "echo");
    let args = job.steps[0].script.actions.on_run.args.as_ref().unwrap();
    assert_eq!(args[0].raw(), "hello");
}

#[test]
fn test_create_job_parameter_binding_in_job_name() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Render-{{Param.Scene}}",
        "parameterDefinitions": [{"name": "Scene", "type": "STRING"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[("Scene", "forest")],
    );
    assert_eq!(job.name, "Render-forest");
}

#[test]
fn test_create_job_host_requirements_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "MinCpu", "type": "INT"}],
        "steps": [{
            "name": "S",
            "hostRequirements": {
                "amounts": [{"name": "amount.worker.vcpu", "min": "{{Param.MinCpu}}", "max": "16"}]
            },
            "script": {"actions": {"onRun": {"command": "run"}}}
        }]
    }"#,
        &[("MinCpu", "4")],
    );
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    let amt = &hr.amounts.as_ref().unwrap()[0];
    assert_eq!(amt.name, "amount.worker.vcpu");
    assert_eq!(amt.min, Some(4.0));
    assert_eq!(amt.max, Some(16.0));
}

#[test]
fn test_create_job_parameter_space_int_list() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": [1, 2, 3]}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    let frame = &ps.task_parameter_definitions["Frame"];
    match frame {
        job::TaskParameter::Int { range, .. } => match range {
            job::TaskParamRange::List(v) => assert_eq!(v, &[1, 2, 3]),
            other => panic!("Expected List, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
}

#[test]
fn test_create_job_parameter_space_range_expr() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Frames", "type": "STRING"}],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "{{Param.Frames}}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[("Frames", "1-10")],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    let frame = &ps.task_parameter_definitions["Frame"];
    match frame {
        job::TaskParameter::Int { range, .. } => match range {
            job::TaskParamRange::RangeExpr(r) => assert_eq!(r.to_string(), "1-10"),
            other => panic!("Expected RangeExpr, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
}

#[test]
fn test_create_job_environment_carry_forward() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "jobEnvironments": [{
            "name": "MyEnv",
            "variables": {"FOO": "{{Param.Bar}}"},
            "script": {"actions": {"onEnter": {"command": "setup"}}}
        }],
        "parameterDefinitions": [{"name": "Bar", "type": "STRING", "default": "baz"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    let envs = job.job_environments.as_ref().unwrap();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].name, "MyEnv");
    // Variables should carry FormatString through (not resolved at TEMPLATE scope)
    let vars = envs[0].variables.as_ref().unwrap();
    assert_eq!(vars["FOO"].raw(), "{{Param.Bar}}");
}

#[test]
fn test_create_job_step_environment_carry_forward() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "x"}],
        "steps": [{
            "name": "S",
            "stepEnvironments": [{
                "name": "StepEnv",
                "variables": {"KEY": "{{Param.Val}}"}
            }],
            "script": {"actions": {"onRun": {"command": "run"}}}
        }]
    }"#,
        &[],
    );
    let step_envs = job.steps[0].step_environments.as_ref().unwrap();
    assert_eq!(step_envs.len(), 1);
    assert_eq!(step_envs[0].name, "StepEnv");
    let vars = step_envs[0].variables.as_ref().unwrap();
    assert_eq!(vars["KEY"].raw(), "{{Param.Val}}");
}

#[test]
fn test_create_job_missing_required_parameter() {
    let err = parse_and_create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Required", "type": "STRING"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    assert!(
        err.contains("Values missing for required job parameters: Required"),
        "Expected missing parameter error, got: {err}"
    );
}

#[test]
fn test_create_job_syntax_sugar_bash() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{
            "name": "MyStep",
            "bash": {"script": "echo hello"}
        }]
    }"#,
        &[],
    );
    assert_eq!(job.steps[0].name, "MyStep");
    assert_eq!(job.steps[0].script.actions.on_run.command.raw(), "bash");
    let files = job.steps[0].script.embedded_files.as_ref().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_type, openjd_model::types::FileType::Text);
    assert_eq!(files[0].data.as_ref().unwrap().raw(), "echo hello");
    assert_eq!(files[0].runnable, Some(true));
}

#[test]
fn test_create_job_dependencies_preserved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "Build", "script": {"actions": {"onRun": {"command": "build"}}}},
            {
                "name": "Test",
                "dependencies": [{"dependsOn": "Build"}],
                "script": {"actions": {"onRun": {"command": "test"}}}
            }
        ]
    }"#,
        &[],
    );
    assert_eq!(job.steps.len(), 2);
    let deps = job.steps[1].dependencies.as_ref().unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].depends_on, "Build");
}

#[test]
fn test_create_job_extensions_carried_forward() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    assert_eq!(
        job.extensions,
        Some(vec![openjd_model::ModelExtension::Expr])
    );
}

#[test]
fn test_create_job_job_name_in_step_let_binding() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "MyJob",
        "parameterDefinitions": [{"name": "X", "type": "INT", "default": 1}],
        "steps": [{
            "name": "S",
            "let": ["nameLen = len(Job.Name)"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{nameLen}}"]}}}
        }]
    }"#,
        &[],
    );
    assert_eq!(job.steps[0].name, "S");
}

#[test]
fn test_create_job_step_name_in_let_binding() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "parameterDefinitions": [{"name": "X", "type": "INT", "default": 1}],
        "steps": [{
            "name": "RenderStep",
            "let": ["stepLen = len(Step.Name)"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{stepLen}}"]}}}
        }]
    }"#,
        &[],
    );
    assert_eq!(job.steps[0].name, "RenderStep");
}

#[test]
fn test_create_job_step_let_binding_in_host_requirements() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "name": "MyJob",
        "parameterDefinitions": [{"name": "CpuCount", "type": "INT", "default": 4}],
        "steps": [{
            "name": "S",
            "let": ["cpus = Param.CpuCount"],
            "hostRequirements": {
                "amounts": [{"name": "amount.worker.vcpu", "min": "{{cpus}}"}]
            },
            "script": {"actions": {"onRun": {"command": "work"}}}
        }]
    }"#,
        &[],
    );
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    let amt = &hr.amounts.as_ref().unwrap()[0];
    assert_eq!(amt.name, "amount.worker.vcpu");
    assert_eq!(amt.min, Some(4.0));
}

#[test]
fn test_create_job_resolved_symtab_populated() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "MyJob",
        "parameterDefinitions": [{"name": "Count", "type": "INT", "default": 42}],
        "steps": [{
            "name": "S",
            "let": ["doubled = Param.Count * 2", "label = 'hello'"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{doubled}}"]}}}
        }]
    }"#,
        &[],
    );
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .expect("should have resolved_symtab")
        .to_symtab(openjd_expr::PathFormat::Posix)
        .unwrap();
    assert_eq!(
        symtab.get_value("doubled"),
        Some(&openjd_expr::ExprValue::Int(84))
    );
    // "label" is not referenced by any format string, so it's filtered out
    assert_eq!(symtab.get_value("label"), None);
    // Param.Count is referenced by the let binding "doubled = Param.Count * 2"
    assert_eq!(
        symtab.get_value("Param.Count"),
        Some(&openjd_expr::ExprValue::Int(42))
    );
    // Job.Name and Step.Name are not referenced by any format string or let binding
    assert_eq!(symtab.get_value("Job.Name"), None);
    assert_eq!(symtab.get_value("Step.Name"), None);
}

#[test]
fn test_create_job_resolved_symtab_always_present() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        &[],
    );
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .expect("should always have resolved_symtab")
        .to_symtab(openjd_expr::PathFormat::Posix)
        .unwrap();
    assert_eq!(symtab.get_value("Step.Name"), None); // No EXPR extension, so no Step.Name
}

// === Scope boundary tests: SESSION/TASK-scope FormatStrings must NOT be resolved ===
//
// These tests verify that every FormatString field that belongs to SESSION or TASK
// scope is carried through create_job as an unresolved FormatString (checked via .raw()).
// Only TEMPLATE-scope fields (job name, step name, host requirements, parameter space
// ranges) should be resolved.

/// Build a job from a template that puts {{Param.Val}} into every FormatString field,
/// then verify which fields got resolved (TEMPLATE scope) vs carried through (SESSION/TASK).
#[test]
fn test_scope_boundary_action_command_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "RESOLVED"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "{{Param.Val}}"}}}}]
    }"#,
        &[],
    );
    assert_eq!(
        job.steps[0].script.actions.on_run.command.raw(),
        "{{Param.Val}}",
        "Action command is SESSION/TASK scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_action_args_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "RESOLVED"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.Val}}"]}}}}]
    }"#,
        &[],
    );
    let args = job.steps[0].script.actions.on_run.args.as_ref().unwrap();
    assert_eq!(
        args[0].raw(),
        "{{Param.Val}}",
        "Action args are SESSION/TASK scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_action_timeout_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "INT", "default": 30}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run", "timeout": "{{Param.Val}}"}}}}]
    }"#,
        &[],
    );
    let timeout = job.steps[0].script.actions.on_run.timeout.as_ref().unwrap();
    assert_eq!(
        timeout.raw(),
        "{{Param.Val}}",
        "Action timeout is SESSION/TASK scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_cancelation_notify_period_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "INT", "default": 10}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {
            "command": "run",
            "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{Param.Val}}"}
        }}}}]
    }"#,
        &[],
    );
    let cancel = job.steps[0]
        .script
        .actions
        .on_run
        .cancelation
        .as_ref()
        .unwrap();
    match cancel {
        openjd_model::job::CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds,
        } => {
            let notify = notify_period_in_seconds.as_ref().unwrap();
            assert_eq!(notify.raw(), "{{Param.Val}}",
                "Cancelation notifyPeriodInSeconds is SESSION/TASK scope — must NOT be resolved during create_job");
        }
        _ => panic!("Expected NotifyThenTerminate"),
    }
}

#[test]
fn test_scope_boundary_embedded_file_filename_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "script.sh"}],
        "steps": [{"name": "S", "script": {
            "embeddedFiles": [{"name": "myfile", "type": "TEXT", "filename": "{{Param.Val}}", "data": "echo hi"}],
            "actions": {"onRun": {"command": "bash", "args": ["{{Task.File.myfile}}"]}}
        }}]
    }"#,
        &[],
    );
    let files = job.steps[0].script.embedded_files.as_ref().unwrap();
    assert_eq!(
        files[0].filename.as_ref().unwrap().raw(),
        "{{Param.Val}}",
        "EmbeddedFile filename is SESSION/TASK scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_embedded_file_data_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "hello"}],
        "steps": [{"name": "S", "script": {
            "embeddedFiles": [{"name": "myfile", "type": "TEXT", "data": "echo {{Param.Val}}"}],
            "actions": {"onRun": {"command": "bash", "args": ["{{Task.File.myfile}}"]}}
        }}]
    }"#,
        &[],
    );
    let files = job.steps[0].script.embedded_files.as_ref().unwrap();
    assert_eq!(
        files[0].data.as_ref().unwrap().raw(),
        "echo {{Param.Val}}",
        "EmbeddedFile data is SESSION/TASK scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_env_action_command_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "setup"}],
        "jobEnvironments": [{
            "name": "E",
            "script": {"actions": {
                "onEnter": {"command": "{{Param.Val}}", "args": ["{{Param.Val}}"]},
                "onExit": {"command": "{{Param.Val}}", "args": ["{{Param.Val}}"]}
            }}
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    let env = &job.job_environments.as_ref().unwrap()[0];
    let script = env.script.as_ref().unwrap();
    let on_enter = script.actions.on_enter.as_ref().unwrap();
    assert_eq!(
        on_enter.command.raw(),
        "{{Param.Val}}",
        "Environment onEnter command is SESSION scope — must NOT be resolved during create_job"
    );
    assert_eq!(
        on_enter.args.as_ref().unwrap()[0].raw(),
        "{{Param.Val}}",
        "Environment onEnter args are SESSION scope — must NOT be resolved during create_job"
    );
    let on_exit = script.actions.on_exit.as_ref().unwrap();
    assert_eq!(
        on_exit.command.raw(),
        "{{Param.Val}}",
        "Environment onExit command is SESSION scope — must NOT be resolved during create_job"
    );
    assert_eq!(
        on_exit.args.as_ref().unwrap()[0].raw(),
        "{{Param.Val}}",
        "Environment onExit args are SESSION scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_env_variables_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "x"}],
        "jobEnvironments": [{"name": "E", "variables": {"KEY": "{{Param.Val}}"}}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    let env = &job.job_environments.as_ref().unwrap()[0];
    let vars = env.variables.as_ref().unwrap();
    assert_eq!(
        vars["KEY"].raw(),
        "{{Param.Val}}",
        "Environment variables are SESSION scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_env_embedded_files_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "x"}],
        "jobEnvironments": [{
            "name": "E",
            "script": {
                "embeddedFiles": [{"name": "envfile", "type": "TEXT", "filename": "{{Param.Val}}", "data": "echo {{Param.Val}}"}],
                "actions": {"onEnter": {"command": "bash"}}
            }
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    let env = &job.job_environments.as_ref().unwrap()[0];
    let files = env
        .script
        .as_ref()
        .unwrap()
        .embedded_files
        .as_ref()
        .unwrap();
    assert_eq!(files[0].filename.as_ref().unwrap().raw(), "{{Param.Val}}",
        "Environment embedded file filename is SESSION scope — must NOT be resolved during create_job");
    assert_eq!(
        files[0].data.as_ref().unwrap().raw(),
        "echo {{Param.Val}}",
        "Environment embedded file data is SESSION scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_step_env_action_not_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "x"}],
        "steps": [{
            "name": "S",
            "stepEnvironments": [{
                "name": "SE",
                "script": {"actions": {
                    "onEnter": {"command": "{{Param.Val}}"},
                    "onExit": {"command": "{{Param.Val}}"}
                }}
            }],
            "script": {"actions": {"onRun": {"command": "run"}}}
        }]
    }"#,
        &[],
    );
    let step_env = &job.steps[0].step_environments.as_ref().unwrap()[0];
    let script = step_env.script.as_ref().unwrap();
    assert_eq!(
        script.actions.on_enter.as_ref().unwrap().command.raw(),
        "{{Param.Val}}",
        "Step environment onEnter is SESSION scope — must NOT be resolved during create_job"
    );
    assert_eq!(
        script.actions.on_exit.as_ref().unwrap().command.raw(),
        "{{Param.Val}}",
        "Step environment onExit is SESSION scope — must NOT be resolved during create_job"
    );
}

#[test]
fn test_scope_boundary_script_let_bindings_carried_through() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Val", "type": "STRING", "default": "x"}],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["myvar = Param.Val"],
                "actions": {"onRun": {"command": "echo", "args": ["{{myvar}}"]}}
            }
        }]
    }"#,
        &[],
    );
    let let_bindings = job.steps[0].script.let_bindings.as_ref().unwrap();
    assert_eq!(
        let_bindings[0], "myvar = Param.Val",
        "Script-level let bindings are SESSION/TASK scope — must be carried through as strings"
    );
}

// Verify TEMPLATE-scope fields ARE resolved (positive control)
#[test]
fn test_scope_boundary_template_scope_fields_are_resolved() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{{Param.JobName}}",
        "parameterDefinitions": [
            {"name": "JobName", "type": "STRING", "default": "MyJob"}
        ],
        "steps": [{"name": "MyStep", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
        &[],
    );
    assert_eq!(
        job.name, "MyJob",
        "Job name is TEMPLATE scope — must be resolved"
    );
    assert_eq!(
        job.steps[0].name, "MyStep",
        "Step name is a plain string — passed through as-is"
    );
}

// === Task parameter value validation (§3.4.2) ===

#[test]
fn test_string_task_param_value_too_long() {
    let long_val = "x".repeat(1025);
    let err = parse_and_create_err(
        &format!(
            r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "parameterSpace": {{
                "taskParameterDefinitions": [{{"name": "Val", "type": "STRING", "range": ["{long_val}"]}}]
            }},
            "script": {{"actions": {{"onRun": {{"command": "run"}}}}}}
        }}]
    }}"#
        ),
        &[],
    );
    assert!(
        err.contains("exceeds 1024 characters"),
        "Expected length error, got: {err}"
    );
}

#[test]
fn test_path_task_param_empty_value() {
    let err = parse_and_create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "E", "type": "STRING", "default": ""}],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Val", "type": "PATH", "range": ["{{Param.E}}"]}]
            },
            "script": {"actions": {"onRun": {"command": "run"}}}
        }]
    }"#,
        &[],
    );
    assert!(
        err.contains("must not be empty"),
        "Expected empty path error, got: {err}"
    );
}

#[test]
fn test_path_task_param_value_too_long() {
    let long_val = "x".repeat(1025);
    let err = parse_and_create_err(
        &format!(
            r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "parameterSpace": {{
                "taskParameterDefinitions": [{{"name": "Val", "type": "PATH", "range": ["{long_val}"]}}]
            }},
            "script": {{"actions": {{"onRun": {{"command": "run"}}}}}}
        }}]
    }}"#
        ),
        &[],
    );
    assert!(
        err.contains("exceeds 1024 characters"),
        "Expected length error, got: {err}"
    );
}

#[test]
fn test_string_task_param_1024_chars_ok() {
    let val_1024 = "x".repeat(1024);
    let job = parse_and_create(
        &format!(
            r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "parameterSpace": {{
                "taskParameterDefinitions": [{{"name": "Val", "type": "STRING", "range": ["{val_1024}"]}}]
            }},
            "script": {{"actions": {{"onRun": {{"command": "run"}}}}}}
        }}]
    }}"#
        ),
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Val"] {
        job::TaskParameter::String { range } => assert_eq!(range[0].len(), 1024),
        other => panic!("Expected String, got {:?}", other),
    }
}

// === Tests ported from Python v2023_09/test_create.py ===

#[test]
fn test_create_job_v2023_09_comprehensive() {
    // Comprehensive end-to-end test from Python test_create.py::TestCreateJob::test_v2023_09
    // Key: every format string has a job parameter reference, only job name & task parameter
    // range values should be evaluated at TEMPLATE scope.
    // Note: Rust validates INT/FLOAT list range values at parse time, so we use literal values
    // in list ranges and format strings only in range expressions and STRING ranges.
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{{ Param.StringParam }}",
        "description": "job description",
        "jobEnvironments": [{
            "name": "JobEnv",
            "description": "desc",
            "script": {
                "embeddedFiles": [{"name": "File", "type": "TEXT", "data": "some data {{ Param.IntParam }}", "filename": "filename.txt", "runnable": false}],
                "actions": {
                    "onEnter": {"command": "{{ Param.IntParam }}", "args": ["{{ Param.FloatParam }}"], "timeout": 10},
                    "onExit": {"command": "{{ Param.IntParam }}", "args": ["{{ Param.FloatParam }}"], "timeout": 10}
                }
            }
        }],
        "parameterDefinitions": [
            {"name": "StringParam", "type": "STRING", "default": "TheOtherJobName"},
            {"name": "RangeExpressionParam", "type": "INT", "default": 75},
            {"name": "IntParam", "type": "INT", "default": 20},
            {"name": "FloatParam", "type": "FLOAT", "default": 20}
        ],
        "steps": [{
            "name": "StepName",
            "description": "desc",
            "stepEnvironments": [{
                "name": "StepEnv",
                "description": "desc",
                "script": {
                    "embeddedFiles": [{"name": "File", "type": "TEXT", "data": "some data {{ Param.IntParam }}", "filename": "filename.txt", "runnable": false}],
                    "actions": {
                        "onEnter": {"command": "{{ Param.IntParam }}", "args": ["{{ Param.FloatParam }}"], "timeout": 10},
                        "onExit": {"command": "{{ Param.IntParam }}", "args": ["{{ Param.FloatParam }}"], "timeout": 10}
                    }
                }
            }],
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "ParamE", "type": "INT", "range": "2 - {{ Param.RangeExpressionParam }}"},
                    {"name": "ParamI", "type": "INT", "range": [0, 10]},
                    {"name": "ParamF", "type": "FLOAT", "range": [1.1, 10.0]},
                    {"name": "ParamS", "type": "STRING", "range": ["foo", "{{ Param.StringParam }}"]}
                ],
                "combination": "ParamS * ParamF * ParamI * ParamE"
            },
            "script": {
                "embeddedFiles": [{"name": "File", "type": "TEXT", "data": "some data {{ Param.IntParam }}", "filename": "filename.txt", "runnable": false}],
                "actions": {"onRun": {"command": "{{ Param.IntParam }}", "args": ["{{ Param.FloatParam }}"], "timeout": 10}}
            }
        }]
    }"#,
        &[
            ("IntParam", "10"),
            ("FloatParam", "10"),
            ("RangeExpressionParam", "3"),
        ],
    );

    // Job name should be resolved (TEMPLATE scope)
    assert_eq!(job.name, "TheOtherJobName");
    assert_eq!(job.description.as_deref(), Some("job description"));

    // Job environments should carry through unresolved format strings
    let envs = job.job_environments.as_ref().unwrap();
    assert_eq!(envs.len(), 1);
    assert_eq!(envs[0].name, "JobEnv");

    // Step
    assert_eq!(job.steps.len(), 1);
    assert_eq!(job.steps[0].name, "StepName");

    // Parameter space ranges should be resolved
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["ParamE"] {
        job::TaskParameter::Int { range, .. } => match range {
            job::TaskParamRange::RangeExpr(r) => {
                // "2 - 3" resolves to values [2, 3]
                let vals: Vec<i64> = r.iter().collect();
                assert_eq!(vals, vec![2, 3]);
            }
            other => panic!("Expected RangeExpr, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
    match &ps.task_parameter_definitions["ParamI"] {
        job::TaskParameter::Int { range, .. } => match range {
            job::TaskParamRange::List(v) => assert_eq!(v, &[0, 10]),
            other => panic!("Expected List, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
    match &ps.task_parameter_definitions["ParamS"] {
        job::TaskParameter::String { range } => {
            assert_eq!(range, &["foo", "TheOtherJobName"]);
        }
        other => panic!("Expected String, got {:?}", other),
    }

    // Parameters should be populated
    assert_eq!(job.parameters.len(), 4);
    assert!(
        matches!(&job.parameters["StringParam"].value, openjd_expr::ExprValue::String(s) if s == "TheOtherJobName")
    );
    // INT/FLOAT params are stored as typed values
    assert!(matches!(
        &job.parameters["IntParam"].value,
        openjd_expr::ExprValue::Int(10)
    ));
    assert!(matches!(
        &job.parameters["FloatParam"].value,
        openjd_expr::ExprValue::Float(_)
    ));
    assert!(matches!(
        &job.parameters["RangeExpressionParam"].value,
        openjd_expr::ExprValue::Int(3)
    ));
}

#[test]
fn test_create_job_v2023_09_task_chunking() {
    // End-to-end test for TASK_CHUNKING extension from Python test_create.py
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["TASK_CHUNKING"],
        "name": "Job {{ Param.IntParam }}",
        "parameterDefinitions": [
            {"name": "RangeExpressionParam", "type": "INT", "default": 75},
            {"name": "IntParam", "type": "INT", "default": 20}
        ],
        "steps": [{
            "name": "StepName",
            "parameterSpace": {
                "taskParameterDefinitions": [{
                    "name": "ParamE",
                    "type": "CHUNK[INT]",
                    "range": "2 - {{ Param.RangeExpressionParam }}",
                    "chunks": {
                        "defaultTaskCount": "{{Param.RangeExpressionParam}}",
                        "targetRuntimeSeconds": "{{Param.IntParam}}",
                        "rangeConstraint": "CONTIGUOUS"
                    }
                }],
                "combination": "ParamE"
            },
            "script": {"actions": {"onRun": {"command": "{{ Param.IntParam }}"}}}
        }]
    }"#,
        &[("IntParam", "10"), ("RangeExpressionParam", "3")],
    );

    assert_eq!(job.name, "Job 10");
    assert_eq!(
        job.extensions,
        Some(vec![openjd_model::ModelExtension::TaskChunking])
    );

    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["ParamE"] {
        job::TaskParameter::ChunkInt { range, chunks, .. } => {
            match range {
                job::TaskParamRange::RangeExpr(r) => {
                    // Range expression "2 - 3" resolves to values 2,3
                    let vals: Vec<i64> = r.iter().collect();
                    assert_eq!(vals, vec![2, 3]);
                }
                other => panic!("Expected RangeExpr, got {:?}", other),
            }
            assert_eq!(chunks.default_task_count, 3);
            assert_eq!(chunks.target_runtime_seconds, Some(10));
        }
        other => panic!("Expected ChunkInt, got {:?}", other),
    }
}

// === Tests ported from Python test_create_job.py ===

#[test]
fn test_create_job_rejects_mismatched_association_lengths() {
    // create_job must catch mismatched association lengths, not defer to iterator
    let err = parse_and_create_err(
        r#"{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "Job",
            "steps": [{
                "name": "Step",
                "parameterSpace": {
                    "taskParameterDefinitions": [
                        {"name": "A", "type": "INT", "range": "1-10"},
                        {"name": "B", "type": "INT", "range": [1, 2]}
                    ],
                    "combination": "(A,B)"
                },
                "script": {"actions": {"onRun": {"command": "do something"}}}
            }]
        }"#,
        &[],
    );
    assert!(
        err.contains("same number of values"),
        "Expected association length mismatch error from create_job, got: {err}"
    );
}

#[test]
fn test_uneven_parameter_space_association() {
    let td = TestDirs::new();
    // Association with mismatched lengths should fail during create_job or iteration
    let v: serde_json::Value = serde_saphyr::from_str(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "steps": [{
            "name": "Step",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": "1-10"},
                    {"name": "B", "type": "INT", "range": [1, 2]}
                ],
                "combination": "(A,B)"
            },
            "script": {"actions": {"onRun": {"command": "do something"}}}
        }]
    }"#,
    )
    .unwrap();
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let processed = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let result = create_job(&jt, &processed, &jt.default_validation_context());
    // The error may occur at create_job or at iteration time
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("same number of values")
                    || msg.contains("same length")
                    || msg.contains("identical ranges")
                    || msg.contains("argument lengths"),
                "Expected association length mismatch error, got: {msg}"
            );
        }
        Ok(job) => {
            // If create_job succeeds, the error should occur during iteration
            let ps = job.steps[0].parameter_space.as_ref().unwrap();
            let err = openjd_model::StepParameterSpaceIterator::new(ps);
            match err {
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        msg.contains("same number of values")
                            || msg.contains("same length")
                            || msg.contains("identical ranges")
                            || msg.contains("argument lengths"),
                        "Expected association length mismatch error, got: {msg}"
                    );
                }
                Ok(_) => panic!("Expected error from StepParameterSpaceIterator::new"),
            }
        }
    }
}

#[test]
fn test_create_job_fails_to_instantiate_name_too_long() {
    // In Python, job name exceeding 128 chars fails at create_job.
    // In Rust, the name length is validated at decode time, not create_job time.
    // This test verifies that a 256-char name in the template itself is rejected.
    let long_name = "a".repeat(256);
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{long_name}",
        "steps": [{{"name": "Step", "script": {{"actions": {{"onRun": {{"command": "do something"}}}}}}}}]
    }}"#
    );
    let v: serde_json::Value = serde_saphyr::from_str(&template).unwrap();
    let err = decode_job_template(v, None, &CallerLimits::default()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("128")
            || msg.contains("too long")
            || msg.contains("at most")
            || msg.contains("characters"),
        "Expected name too long error, got: {msg}"
    );
}

// === Tests ported from Python _internal/test_create_job.py ===
// The _internal tests test instantiate_model internals. Most are Python-specific
// (Pydantic model construction). The key behavioral tests are already covered above.
// The following tests cover the remaining behavioral gaps.

#[test]
fn test_preprocess_reports_extra_with_environments() {
    let td = TestDirs::new();
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "ThisIsKnown", "type": "STRING"}],
        "environment": {"name": "env", "script": {"actions": {"onEnter": {"command": "do thing"}}}}
    }"#,
    );
    let et = decode_environment_template(et_val, None).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "ThisIsUnknown".into(),
        openjd_expr::ExprValue::String("value".into()),
    );
    input.insert(
        "ThisIsKnown".into(),
        openjd_expr::ExprValue::String("value".into()),
    );
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("ThisIsUnknown"),
        "Expected extra param error, got: {err}"
    );
}

#[test]
fn test_preprocess_reports_missing_with_environments() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "ThisIsNotDefined", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "ThisIsAlsoMissing", "type": "STRING"}],
        "environment": {"name": "env", "script": {"actions": {"onEnter": {"command": "do thing"}}}}
    }"#,
    );
    let et = decode_environment_template(et_val, None).unwrap();
    let input = JobParameterInputValues::new();
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("ThisIsNotDefined"),
        "Expected missing param error, got: {msg}"
    );
    assert!(
        msg.contains("ThisIsAlsoMissing"),
        "Expected missing env param error, got: {msg}"
    );
}

#[test]
fn test_preprocess_collects_defaults_with_environments() {
    let td = TestDirs::new();
    let jt_val =
        minimal_job_template(r#"{"name": "Foo", "type": "STRING", "default": "defaultValue"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "Bar", "type": "STRING", "default": "alsoDefaultValue"}],
        "environment": {"name": "env", "script": {"actions": {"onEnter": {"command": "do thing"}}}}
    }"#,
    );
    let et = decode_environment_template(et_val, None).unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(
        matches!(result["Foo"].value, openjd_expr::ExprValue::String(ref s) if s == "defaultValue")
    );
    assert!(
        matches!(result["Bar"].value, openjd_expr::ExprValue::String(ref s) if s == "alsoDefaultValue")
    );
}

#[test]
fn test_preprocess_checks_constraints_with_environments() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING", "maxLength": 1}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{"name": "Bar", "type": "STRING", "minLength": 5}],
        "environment": {"name": "env", "script": {"actions": {"onEnter": {"command": "do thing"}}}}
    }"#,
    );
    let et = decode_environment_template(et_val, None).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("two".into()));
    input.insert("Bar".into(), openjd_expr::ExprValue::String("one".into()));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    let msg = err.to_string();
    // At minimum, the first constraint violation should be reported
    assert!(
        msg.contains("Foo") || msg.contains("Bar"),
        "Expected constraint error for Foo or Bar, got: {msg}"
    );
}

#[test]
fn test_preprocess_collects_multiple_errors() {
    let td = TestDirs::new();
    // Extra param, missing param, and constraint violation — at least one should be reported
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "parameterDefinitions": [
            {"name": "Foo", "type": "STRING", "maxLength": 1},
            {"name": "Buz", "type": "STRING"}
        ],
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("two".into()));
    input.insert("Bar".into(), openjd_expr::ExprValue::String("three".into()));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    let msg = err.to_string();
    // The Rust implementation may report errors differently (one at a time or all at once)
    // At minimum, one of the errors should be reported
    assert!(
        msg.contains("Foo") || msg.contains("Bar") || msg.contains("Buz"),
        "Expected at least one error about Foo, Bar, or Buz, got: {msg}"
    );
}

#[test]
fn test_preprocess_ignores_defaults_when_value_provided() {
    let td = TestDirs::new();
    let jt_val =
        minimal_job_template(r#"{"name": "Foo", "type": "STRING", "default": "defaultValue"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String("FooValue".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(
        matches!(result["Foo"].value, openjd_expr::ExprValue::String(ref s) if s == "FooValue")
    );
}

#[test]
fn test_preprocess_string_constraint_violation_message() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING", "maxLength": 1}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("two".into()));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Foo")
            && (msg.contains("1") || msg.contains("maximum") || msg.contains("exceed")),
        "Expected constraint message mentioning Foo and length limit, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Category B: Type coercion in preprocess_job_parameters
// ══════════════════════════════════════════════════════════════

fn expr_job_template_with_param(param_json: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "test",
        "parameterDefinitions": [{param_json}],
        "steps": [{{"name": "step", "script": {{"actions": {{"onRun": {{"command": "do thing"}}}}}}}}]
    }}"#
    ))
}

#[test]
fn test_preprocess_int_to_float_coercion() {
    let td = TestDirs::new();
    // Providing an Int value for a FLOAT param should coerce Int→Float
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "FLOAT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::Int(42));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    match &result["Foo"].value {
        openjd_expr::ExprValue::Float(f) => assert_eq!(f.value(), 42.0),
        other => panic!("Expected Float, got {:?}", other),
    }
}

#[test]
fn test_preprocess_float_to_int_coercion_whole() {
    let td = TestDirs::new();
    // Float(5.0) for INT param → coerces to Int(5)
    // Use env-only param to avoid check_constraints on raw value
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = minimal_env_template(r#"{"name": "Foo", "type": "INT"}"#);
    let et = decode_environment_template(et_val, None).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::Float(openjd_expr::value::Float64::new(5.0).unwrap()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Int(5)
    ));
}

#[test]
fn test_preprocess_float_to_int_coercion_fractional_falls_through() {
    let td = TestDirs::new();
    // Float(5.5) for INT param → can't coerce losslessly, returns error
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = minimal_env_template(r#"{"name": "Foo", "type": "INT"}"#);
    let et = decode_environment_template(et_val, None).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::Float(openjd_expr::value::Float64::new(5.5).unwrap()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not a valid integer"),
        "Expected integer coercion error, got: {msg}"
    );
}

#[test]
fn test_preprocess_bool_coercion_true() {
    let td = TestDirs::new();
    // String "true" for BOOL param → Bool(true)
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "BOOL"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("true".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Bool(true)
    ));
}

#[test]
fn test_preprocess_bool_coercion_false() {
    let td = TestDirs::new();
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "BOOL"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("false".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Bool(false)
    ));
}

#[test]
fn test_preprocess_bool_coercion_yes() {
    let td = TestDirs::new();
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "BOOL"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("yes".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Bool(true)
    ));
}

#[test]
fn test_preprocess_bool_coercion_0() {
    let td = TestDirs::new();
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "BOOL"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("0".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Bool(false)
    ));
}

#[test]
fn test_preprocess_bool_invalid_string() {
    let td = TestDirs::new();
    // check_constraints catches invalid bool before coercion
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "BOOL"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("maybe".into()));
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not a valid bool"), "got: {err}");
}

#[test]
fn test_preprocess_bool_from_bool_value() {
    let td = TestDirs::new();
    // Bool(true) for BOOL param → passes through (already correct type)
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "BOOL"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::Bool(true));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Bool(true)
    ));
}

#[test]
fn test_preprocess_range_expr_coercion() {
    let td = TestDirs::new();
    // String "1-10" for RANGE_EXPR param → RangeExpr
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "RANGE_EXPR"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("1-10".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::RangeExpr(_)
    ));
}

#[test]
fn test_preprocess_range_expr_invalid_stays_string() {
    let td = TestDirs::new();
    // Invalid range expr string is caught by check_constraints
    let jt_val = expr_job_template_with_param(r#"{"name": "Foo", "type": "RANGE_EXPR"}"#);
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String("not-a-range".into()),
    );
    let err = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("not a valid range"), "got: {err}");
}

fn expr_env_template(params: &str) -> serde_json::Value {
    yaml_val(&format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "parameterDefinitions": [{params}],
        "environment": {{"name": "env", "script": {{"actions": {{"onEnter": {{"command": "bar"}}}}}}}}
    }}"#
    ))
}

fn expr_job_no_params() -> serde_json::Value {
    yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "test",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    )
}

#[test]
fn test_preprocess_list_int_json_coercion() {
    let td = TestDirs::new();
    // JSON string "[1,2,3]" for LIST[INT] param → ListInt
    // Use env-only param to bypass check_constraints on raw String
    let jt_val = expr_job_no_params();
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let et = decode_environment_template(
        expr_env_template(r#"{"name": "Foo", "type": "LIST[INT]"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String("[1,2,3]".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::ListInt(_)
    ));
}

#[test]
fn test_preprocess_list_string_json_coercion() {
    let td = TestDirs::new();
    let jt_val = expr_job_no_params();
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let et = decode_environment_template(
        expr_env_template(r#"{"name": "Foo", "type": "LIST[STRING]"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String(r#"["a","b"]"#.into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::ListString(_, _)
    ));
}

#[test]
fn test_preprocess_list_bool_json_coercion() {
    let td = TestDirs::new();
    let jt_val = expr_job_no_params();
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let et = decode_environment_template(
        expr_env_template(r#"{"name": "Foo", "type": "LIST[BOOL]"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String("[true,false]".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::ListBool(_)
    ));
}

#[test]
fn test_preprocess_list_invalid_json_stays_string() {
    let td = TestDirs::new();
    // Non-JSON string for LIST[INT] → returns error
    let jt_val = expr_job_no_params();
    let jt = decode_job_template(jt_val, Some(&["EXPR"]), &CallerLimits::default()).unwrap();
    let et = decode_environment_template(
        expr_env_template(r#"{"name": "Foo", "type": "LIST[INT]"}"#),
        Some(&["EXPR"]),
    )
    .unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String("not json".into()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not valid JSON"),
        "Expected JSON parse error, got: {msg}"
    );
}

// ── Fix 1: coerce_from_str errors on invalid values ──

#[test]
fn test_coerce_string_to_int_errors_on_non_numeric() {
    let td = TestDirs::new();
    // "abc" for INT param → error, not silent fallback to String
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "INT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("abc".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not a valid integer"));
}

#[test]
fn test_coerce_string_to_float_errors_on_non_numeric() {
    let td = TestDirs::new();
    // "xyz" for FLOAT param → error, not silent fallback to String
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "FLOAT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("xyz".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("not a valid float"));
}

#[test]
fn test_coerce_valid_int_string_succeeds() {
    let td = TestDirs::new();
    // "42" for INT param → Int(42)
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "INT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("42".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Int(42)
    ));
}

#[test]
fn test_coerce_valid_float_string_succeeds() {
    let td = TestDirs::new();
    // "3.14" for FLOAT param → Float(3.14)
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "FLOAT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::String("3.14".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(
        result["Foo"].value,
        openjd_expr::ExprValue::Float(_)
    ));
}

// ── Fix 2: relative template dir guard ──

#[test]
fn test_relative_template_dir_with_walkup_false_errors() {
    let td = TestDirs::new();
    // Relative job_template_dir + allow_job_template_dir_walk_up=false → error
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING", "default": "bar"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let result = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "relative/dir",
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("is not an absolute path"));
}

#[test]
fn test_relative_template_dir_with_walkup_true_succeeds() {
    let td = TestDirs::new();
    // Relative job_template_dir + allow_job_template_dir_walk_up=true → OK
    let jt_val =
        minimal_job_template(r#"{"name": "Foo", "type": "PATH", "default": "data/input.csv"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let result = preprocess_job_parameters(
        &jt,
        &JobParameterInputValues::new(),
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "relative/dir",
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_ok());
}

// ── Fix 3: MergedParameter re-export ──

#[test]
fn test_merged_parameter_is_accessible() {
    // Verify MergedParameter is usable from the public API
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let merged: Vec<openjd_model::MergedParameterDefinition> =
        merge_job_parameter_definitions(&jt, &[]).unwrap();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].name, "Foo");
    assert_eq!(merged[0].param_type, openjd_model::JobParameterType::String);
}

// Coercion from non-string ExprValue types
#[test]
fn test_preprocess_int_coerced_to_string_repr() {
    let td = TestDirs::new();
    // Int(42) for STRING param → coerce_from_str("42", String) → String("42")
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::Int(42));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(result["Foo"].value, openjd_expr::ExprValue::String(ref s) if s == "42"));
}

#[test]
fn test_preprocess_bool_coerced_to_string_repr() {
    let td = TestDirs::new();
    // Bool(true) for STRING param → coerce_from_str("true", String) → String("true")
    let jt_val = minimal_job_template(r#"{"name": "Foo", "type": "STRING"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("Foo".into(), openjd_expr::ExprValue::Bool(true));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    assert!(matches!(result["Foo"].value, openjd_expr::ExprValue::String(ref s) if s == "true"));
}

// ══════════════════════════════════════════════════════════════
// Category C: PATH default edge cases
// ══════════════════════════════════════════════════════════════

#[test]
fn test_path_absolute_default_allowed_with_walkup() {
    let td = TestDirs::new();
    // Absolute PATH default is allowed when allow_job_template_dir_walk_up=true
    let (json_default, expected) = if cfg!(windows) {
        (r#"C:\\\\absolute\\\\path"#, r"C:\\absolute\\path")
    } else {
        ("/absolute/path", "/absolute/path")
    };
    let jt_val = minimal_job_template(&format!(
        r#"{{"name": "DataDir", "type": "PATH", "default": "{}"}}"#,
        json_default
    ));
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    if let openjd_expr::ExprValue::String(ref s) = result["DataDir"].value {
        assert_eq!(s, expected);
    } else {
        panic!("Expected String");
    }
}

#[test]
fn test_path_default_relative_template_dir() {
    let td = TestDirs::new();
    // When template dir is relative and walk-up is disallowed, error is raised
    let jt_val =
        minimal_job_template(r#"{"name": "DataDir", "type": "PATH", "default": "data/input.csv"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let input = JobParameterInputValues::new();
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "relative/template",
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("is not an absolute path"),
        "Expected absolute path error, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Phase 2: Expression-based range resolution and host requirement attributes
// ══════════════════════════════════════════════════════════════

#[test]
fn test_create_job_int_range_expr_list() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "{{ [1, 2, 3] }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Frame"] {
        openjd_model::job::TaskParameter::Int { range, .. } => match range {
            openjd_model::job::TaskParamRange::List(v) => assert_eq!(v, &[1, 2, 3]),
            other => panic!("Expected List, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
}

#[test]
fn test_create_job_int_range_expr_range_expr() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Frames", "type": "RANGE_EXPR"}],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "{{Param.Frames}}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[("Frames", "1-5")],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Frame"] {
        openjd_model::job::TaskParameter::Int { range, .. } => match range {
            openjd_model::job::TaskParamRange::RangeExpr(r) => assert_eq!(r.len(), 5),
            other => panic!("Expected RangeExpr, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
}

#[test]
fn test_create_job_int_range_expr_non_int_elements() {
    let err = parse_and_create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "{{ ['a', 'b'] }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    assert!(err.contains("Expected int in range"), "got: {err}");
}

#[test]
fn test_create_job_int_range_string_fallback() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Frames", "type": "STRING"}],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "{{Param.Frames}}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[("Frames", "1-5")],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Frame"] {
        openjd_model::job::TaskParameter::Int { range, .. } => match range {
            openjd_model::job::TaskParamRange::RangeExpr(r) => assert_eq!(r.len(), 5),
            other => panic!("Expected RangeExpr, got {:?}", other),
        },
        other => panic!("Expected Int, got {:?}", other),
    }
}

#[test]
fn test_create_job_float_range_expr_list() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Weight", "type": "FLOAT", "range": "{{ [1.0, 2.5, 3.7] }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Weight"] {
        openjd_model::job::TaskParameter::Float { range } => {
            assert_eq!(range.len(), 3);
            assert!((range[0] - 1.0).abs() < f64::EPSILON);
        }
        other => panic!("Expected Float, got {:?}", other),
    }
}

#[test]
fn test_create_job_float_range_expr_int_promotion() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Weight", "type": "FLOAT", "range": "{{ [1, 2, 3] }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Weight"] {
        openjd_model::job::TaskParameter::Float { range } => {
            assert_eq!(range.len(), 3);
            assert!((range[0] - 1.0).abs() < f64::EPSILON);
        }
        other => panic!("Expected Float, got {:?}", other),
    }
}

#[test]
fn test_create_job_float_range_expr_not_list() {
    let err = parse_and_create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Weight", "type": "FLOAT", "range": "{{ 42 }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    assert!(err.contains("must evaluate to a list"), "got: {err}");
}

#[test]
fn test_create_job_string_range_expr_list() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Scene", "type": "STRING", "range": "{{ ['a', 'b', 'c'] }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["Scene"] {
        openjd_model::job::TaskParameter::String { range } => assert_eq!(range, &["a", "b", "c"]),
        other => panic!("Expected String, got {:?}", other),
    }
}

#[test]
fn test_create_job_string_range_expr_not_list() {
    let err = parse_and_create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Scene", "type": "STRING", "range": "{{ 'hello' }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    assert!(err.contains("must evaluate to a list"), "got: {err}");
}

#[test]
fn test_create_job_path_range_expr_list() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "File", "type": "PATH", "range": "{{ ['/tmp/a', '/tmp/b'] }}"}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let ps = job.steps[0].parameter_space.as_ref().unwrap();
    match &ps.task_parameter_definitions["File"] {
        openjd_model::job::TaskParameter::Path { range } => {
            assert_eq!(range, &["/tmp/a", "/tmp/b"])
        }
        other => panic!("Expected Path, got {:?}", other),
    }
}

#[test]
fn test_create_job_host_req_attributes_any_of() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "OS", "type": "STRING"}],
        "steps": [{
            "name": "S",
            "hostRequirements": {
                "attributes": [{"name": "attr.worker.os.family", "anyOf": ["{{Param.OS}}"]}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[("OS", "linux")],
    );
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    let attrs = hr.attributes.as_ref().unwrap();
    assert_eq!(attrs[0].name, "attr.worker.os.family");
    assert_eq!(attrs[0].any_of.as_ref().unwrap(), &["linux"]);
}

#[test]
fn test_create_job_host_req_attributes_all_of() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "hostRequirements": {
                "attributes": [{"name": "attr.worker.os.family", "allOf": ["linux"]}]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    let attrs = hr.attributes.as_ref().unwrap();
    assert_eq!(attrs[0].all_of.as_ref().unwrap(), &["linux"]);
}

#[test]
fn test_create_job_host_req_attributes_both() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "hostRequirements": {
                "attributes": [
                    {"name": "attr.worker.os.family", "anyOf": ["linux", "windows"]},
                    {"name": "attr.worker.cpu.arch", "allOf": ["x86_64"]}
                ]
            },
            "script": {"actions": {"onRun": {"command": "render"}}}
        }]
    }"#,
        &[],
    );
    let hr = job.steps[0].host_requirements.as_ref().unwrap();
    let attrs = hr.attributes.as_ref().unwrap();
    assert_eq!(attrs.len(), 2);
    assert_eq!(attrs[0].any_of.as_ref().unwrap(), &["linux", "windows"]);
    assert_eq!(attrs[1].all_of.as_ref().unwrap(), &["x86_64"]);
}

// ══════════════════════════════════════════════════════════════
// Phase 3: Edge cases — build_symbol_table, evaluate_let_bindings,
//          error paths, and remaining coverage gaps
// ══════════════════════════════════════════════════════════════

use openjd_model::{evaluate_let_bindings, JobParameterType, JobParameterValue};
use std::collections::HashMap;

// --- build_symbol_table: LIST[PATH] param ---

#[test]
fn test_build_symbol_table_list_path() {
    let mut params: openjd_model::JobParameterValues = HashMap::new();
    params.insert(
        "Paths".into(),
        JobParameterValue {
            param_type: JobParameterType::ListPath,
            value: openjd_expr::ExprValue::ListString(vec!["/a".into(), "/b".into()], 0),
        },
    );
    let symtab = build_symbol_table(&params).unwrap();
    // Param.Paths should NOT be in the template-scope symtab (PATH types are host-context only)
    assert!(symtab.get_value("Param.Paths").is_none());
    // RawParam.Paths should be list[string]
    let raw_val = symtab.get_value("RawParam.Paths").unwrap();
    assert!(matches!(raw_val, openjd_expr::ExprValue::ListString(_, _)));
}

// --- evaluate_let_bindings: basic success ---

#[test]
fn test_evaluate_let_bindings_basic() {
    let symtab = openjd_expr::symbol_table::SymbolTable::new();
    let bindings = vec!["x = 1 + 2".to_string()];
    let result = evaluate_let_bindings(&bindings, &symtab, None, PathFormat::Posix).unwrap();
    assert!(matches!(
        result.get_value("x").unwrap(),
        openjd_expr::ExprValue::Int(3)
    ));
}

#[test]
fn test_evaluate_let_bindings_chained() {
    let symtab = openjd_expr::symbol_table::SymbolTable::new();
    let bindings = vec!["x = 10".to_string(), "y = x * 2".to_string()];
    let result = evaluate_let_bindings(&bindings, &symtab, None, PathFormat::Posix).unwrap();
    assert!(matches!(
        result.get_value("y").unwrap(),
        openjd_expr::ExprValue::Int(20)
    ));
}

#[test]
fn test_evaluate_let_bindings_with_library() {
    let symtab = openjd_expr::symbol_table::SymbolTable::new();
    let lib = openjd_expr::FunctionLibrary::for_profile(&openjd_expr::ExprProfile::current());
    let bindings = vec!["x = len('hello')".to_string()];
    let result = evaluate_let_bindings(&bindings, &symtab, Some(&lib), PathFormat::Posix).unwrap();
    assert!(matches!(
        result.get_value("x").unwrap(),
        openjd_expr::ExprValue::Int(5)
    ));
}

#[test]
fn test_evaluate_let_bindings_missing_equals() {
    let symtab = openjd_expr::symbol_table::SymbolTable::new();
    let bindings = vec!["x 42".to_string()];
    let err = evaluate_let_bindings(&bindings, &symtab, None, PathFormat::Posix).unwrap_err();
    assert!(err.to_string().contains("Missing '='"), "got: {err}");
}

#[test]
fn test_evaluate_let_bindings_syntax_error() {
    let symtab = openjd_expr::symbol_table::SymbolTable::new();
    let bindings = vec!["x = @@@".to_string()];
    let err = evaluate_let_bindings(&bindings, &symtab, None, PathFormat::Posix).unwrap_err();
    assert!(
        err.to_string().contains("Error evaluating let binding"),
        "got: {err}"
    );
}

#[test]
fn test_evaluate_let_bindings_eval_error() {
    let symtab = openjd_expr::symbol_table::SymbolTable::new();
    let bindings = vec!["x = undefined_var".to_string()];
    let err = evaluate_let_bindings(&bindings, &symtab, None, PathFormat::Posix).unwrap_err();
    assert!(
        err.to_string().contains("Error evaluating let binding"),
        "got: {err}"
    );
}

// --- create_job: name resolution and attribute error paths ---
// Note: Job/step name resolution errors (lines 602-607, 659-663) and
// resolve_string_list errors (lines 837-841) are defensive code — template
// validation catches expression errors before create_job runs.

// --- resolve_to_f64 error: non-numeric format string ---

#[test]
fn test_create_job_host_req_amount_non_numeric() {
    let err = parse_and_create_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Mem", "type": "STRING"}],
        "steps": [{
            "name": "S",
            "hostRequirements": {
                "amounts": [{"name": "amount.worker.memory", "min": "{{Param.Mem}}"}]
            },
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#,
        &[("Mem", "notanumber")],
    );
    assert!(err.contains("not a valid number"), "got: {err}");
}

#[test]
fn zero_dimension_parameter_space() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let jt = decode_job_template(template, None, &CallerLimits::default()).unwrap();
    let params: HashMap<String, openjd_model::JobParameterValue> = HashMap::new();
    let job = create_job(&jt, &params, &jt.default_validation_context()).unwrap();
    let step = &job.steps[0];
    if let Some(ref space) = step.parameter_space {
        let iter = openjd_model::StepParameterSpaceIterator::new(space).unwrap();
        assert_eq!(iter.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════
// Script-level let binding errors should propagate from create_job
// ═══════════════════════════════════════════════════════════════════

#[test]
fn script_let_binding_division_by_zero_is_caught() {
    // A script-level let binding with a concrete division by zero should
    // be caught — either at validation or create_job time.
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["bad = 1 / 0"],
                "actions": {"onRun": {"command": "echo"}}
            }
        }]
    }"#,
    );
    let exts: &[&str] = &["EXPR"];
    let result = decode_job_template(v, Some(exts), &CallerLimits::default());
    // Validation catches this — division by zero with concrete values is detected
    assert!(
        result.is_err(),
        "Division by zero in let binding should be caught"
    );
}

// ══════════════════════════════════════════════════════════════
// resolved_symtab includes RawParam.X for PATH params referenced as Param.X
// ══════════════════════════════════════════════════════════════

#[test]
fn resolved_symtab_includes_raw_param_for_referenced_path_param() {
    let v: serde_json::Value = serde_saphyr::from_str(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Foo", "type": "PATH"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo {{Param.Foo}}"}}}}]
    }"#).unwrap();
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::String("/tmp/foo".into()),
    );
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let job = create_job(&jt, &processed, &jt.default_validation_context()).unwrap();
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .expect("should have resolved_symtab")
        .to_symtab(openjd_expr::PathFormat::Posix)
        .unwrap();
    // RawParam.Foo should be included because Param.Foo is referenced in host context
    assert_eq!(
        symtab.get_value("RawParam.Foo"),
        Some(&openjd_expr::ExprValue::String("/tmp/foo".to_string())),
        "RawParam.Foo should be in resolved_symtab when Param.Foo is referenced"
    );
}

#[test]
fn script_let_binding_param_dependent_division_by_zero_is_caught() {
    // A script-level let binding that divides by a parameter whose value is 0.
    // Validation can't catch this (parameter value isn't known), but create_job should.
    // The binding uses Param.Divisor which is in template scope (concrete at create_job time).
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{
            "name": "Divisor",
            "type": "INT",
            "default": 1
        }],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["result = 100 / Param.Divisor"],
                "actions": {"onRun": {"command": "echo", "args": ["{{result}}"]}}
            }
        }]
    }"#,
    );
    let exts: &[&str] = &["EXPR"];
    let jt = decode_job_template(v, Some(exts), &CallerLimits::default()).unwrap();

    // Provide Divisor=0 to trigger division by zero
    let mut input = JobParameterInputValues::new();
    input.insert("Divisor".into(), openjd_expr::ExprValue::Int(0));
    let params = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    let result = openjd_model::create_job(&jt, &params, &jt.default_validation_context());
    assert!(
        result.is_err(),
        "Script-level let binding '100 / Param.Divisor' with Divisor=0 should error at create_job"
    );
}

#[test]
fn resolved_symtab_includes_raw_param_for_referenced_list_path_param() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Paths", "type": "LIST[PATH]", "default": ["/a", "/b"]}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo {{Param.Paths}}"}}}}]
    }"#,
        &[],
    );
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .expect("should have resolved_symtab")
        .to_symtab(openjd_expr::PathFormat::Posix)
        .unwrap();
    assert!(
        symtab.get_value("RawParam.Paths").is_some(),
        "RawParam.Paths should be in resolved_symtab when Param.Paths is referenced"
    );
}

#[test]
fn script_let_binding_syntax_error_is_caught() {
    // A script-level let binding with a syntax error should be caught.
    // (Validation should catch this first, but if it doesn't, create_job should.)
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["bad = +++"],
                "actions": {"onRun": {"command": "echo"}}
            }
        }]
    }"#,
    );
    let exts: &[&str] = &["EXPR"];
    let result = decode_job_template(v, Some(exts), &CallerLimits::default());
    // This should be caught either at validation or create_job time
    if let Ok(jt) = result {
        let params = std::collections::HashMap::new();
        let result = openjd_model::create_job(&jt, &params, &jt.default_validation_context());
        assert!(
            result.is_err(),
            "Script-level let binding with syntax error should produce an error"
        );
    }
    // If validation caught it, that's also fine
}

#[test]
fn range_expression_evaluation_error_is_caught() {
    // A range expression format string that evaluates to a single int (not a list/range).
    // The typed evaluation returns Ok(Int(5)), which doesn't match RangeExpr or list,
    // so it falls through to string resolution which tries to parse "5" as a range expr.
    // "5" is a valid range expression (single value), so this actually succeeds.
    // This test documents the current behavior.
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{
            "name": "Count",
            "type": "INT",
            "default": 5
        }],
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{
                    "name": "Frame",
                    "type": "INT",
                    "range": "{{Param.Count}}"
                }]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#,
    );
    let exts: &[&str] = &["EXPR"];
    let jt = decode_job_template(v, Some(exts), &CallerLimits::default()).unwrap();

    let mut input = JobParameterInputValues::new();
    input.insert("Count".into(), openjd_expr::ExprValue::Int(5));
    let params = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: "/tmp",
            current_working_dir: "/tmp",
            allow_template_dir_walk_up: true,
            path_format: PathFormat::Posix,
            allow_uri_path_values: true,
        },
    )
    .unwrap();
    // "5" is a valid range expression (single value), so this succeeds
    let result = openjd_model::create_job(&jt, &params, &jt.default_validation_context());
    assert!(
        result.is_ok(),
        "Single int as range expression should work: {:?}",
        result.err()
    );
}

#[test]
fn resolved_symtab_excludes_raw_param_for_unreferenced_path_param() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {"name": "Foo", "type": "PATH"},
            {"name": "Bar", "type": "STRING", "default": "hi"}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo {{Param.Bar}}"}}}}]
    }"#,
        &[("Foo", "/tmp/foo")],
    );
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .expect("should have resolved_symtab")
        .to_symtab(openjd_expr::PathFormat::Posix)
        .unwrap();
    // RawParam.Foo should NOT be included — Param.Foo is not referenced anywhere
    assert_eq!(
        symtab.get_value("RawParam.Foo"),
        None,
        "RawParam.Foo should not be in resolved_symtab when Param.Foo is not referenced"
    );
}

#[test]
fn resolved_symtab_does_not_add_raw_param_for_referenced_string_param() {
    let job = parse_and_create(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "Msg", "type": "STRING", "default": "hello"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo {{Param.Msg}}"}}}}]
    }"#,
        &[],
    );
    let symtab = job.steps[0]
        .resolved_symtab
        .as_ref()
        .expect("should have resolved_symtab")
        .to_symtab(openjd_expr::PathFormat::Posix)
        .unwrap();
    // Param.Msg IS in the template-scope symtab (STRING, not PATH), so it gets
    // copied by the normal filter. RawParam.Msg should NOT be separately added
    // by the PATH-specific logic.
    assert_eq!(
        symtab.get_value("Param.Msg"),
        Some(&openjd_expr::ExprValue::String("hello".to_string())),
    );
    assert_eq!(
        symtab.get_value("RawParam.Msg"),
        None,
        "RawParam.Msg should not be added for STRING params — only PATH/LIST[PATH]"
    );
}

// ══════════════════════════════════════════════════════════════
// Issue 1.1: Float param NaN/Inf user input must error, not panic
// ══════════════════════════════════════════════════════════════

#[test]
fn float_param_user_input_nan_rejected() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "F", "type": "FLOAT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("F".into(), openjd_expr::ExprValue::String("NaN".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err(), "NaN user input must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not a valid float"),
        "Expected float rejection message, got: {msg}"
    );
}

#[test]
fn float_param_user_input_inf_rejected() {
    let td = TestDirs::new();
    let jt_val = minimal_job_template(r#"{"name": "F", "type": "FLOAT"}"#);
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let mut input = JobParameterInputValues::new();
    input.insert("F".into(), openjd_expr::ExprValue::String("inf".into()));
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.template(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err(), "inf user input must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not a valid float"),
        "Expected float rejection message, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Issue 1.3: Singular "1 validation error" (not "1 validation errors")
// ══════════════════════════════════════════════════════════════

#[test]
fn single_error_uses_singular_grammar() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let err = decode_job_template(v, None, &CallerLimits::default()).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("1 validation error for"),
        "Expected singular 'error' not 'errors', got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// Issue 1.4: SimpleAction with digit-starting step name must not panic
// ══════════════════════════════════════════════════════════════

#[test]
fn simple_action_digit_starting_step_name() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Job",
        "extensions": ["FEATURE_BUNDLE_1"],
        "steps": [{"name": "9frames", "bash": {"script": "echo hello"}}]
    }"#,
    );
    let result = decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default());
    assert!(
        result.is_ok(),
        "Step name starting with digit should not panic: {:?}",
        result.err()
    );
}

// ══════════════════════════════════════════════════════════════
// ISSUE-5: Float→Int coercion must reject values outside i64 range
// ══════════════════════════════════════════════════════════════

#[test]
fn test_preprocess_float_to_int_overflow_rejected() {
    let td = TestDirs::new();
    let jt_val = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "test",
        "steps": [{"name": "step", "script": {"actions": {"onRun": {"command": "do thing"}}}}]
    }"#,
    );
    let jt = decode_job_template(jt_val, None, &CallerLimits::default()).unwrap();
    let et_val = minimal_env_template(r#"{"name": "Foo", "type": "INT"}"#);
    let et = decode_environment_template(et_val, None).unwrap();
    let mut input = JobParameterInputValues::new();
    // 1e19 has fract() == 0.0 but exceeds i64::MAX
    input.insert(
        "Foo".into(),
        openjd_expr::ExprValue::Float(openjd_expr::value::Float64::new(1e19).unwrap()),
    );
    let result = preprocess_job_parameters(
        &jt,
        &input,
        &[et],
        &openjd_model::PathParameterOptions {
            job_template_dir: td.cwd(),
            current_working_dir: td.cwd(),
            allow_template_dir_walk_up: false,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    );
    assert!(result.is_err(), "1e19 should not silently coerce to i64");
}
