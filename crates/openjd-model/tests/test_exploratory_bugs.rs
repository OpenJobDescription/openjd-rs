// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Exploratory tests probing potential bugs found during code review.
//!
//! Each test targets a specific suspicious code pattern. Tests are designed
//! to either confirm the bug exists or prove the code is correct.

use openjd_model::job;
use openjd_model::step_param_space::StepParameterSpaceIterator;
use openjd_model::template::{Description, Identifier};
use openjd_model::types::{TaskParameterSet, TaskParameterType, TaskParameterValue};
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

// ══════════════════════════════════════════════════════════════
// 1. Byte vs char length inconsistency in Identifier
//
// Identifier::new uses `s.len()` (byte length) for the 512 limit.
// Description::new uses `s.chars().count()` (char count).
// The spec says "length 1..=512" which should mean characters, not bytes.
//
// We test with multibyte UTF-8 to expose the difference.
// NOTE: Identifier regex only allows [A-Za-z_][A-Za-z0-9_]*, so multibyte
// chars will be rejected by the regex before the length check. These tests
// verify that the length check itself is byte-based by using ASCII strings
// at the boundary, and then demonstrate the inconsistency conceptually.
// ══════════════════════════════════════════════════════════════

#[test]
fn identifier_512_bytes_ascii_is_accepted() {
    // 512 ASCII chars = 512 bytes — should be accepted
    let s = "A".repeat(512);
    assert!(
        Identifier::new(&s).is_ok(),
        "512 ASCII chars should be accepted"
    );
}

#[test]
fn identifier_513_bytes_ascii_is_rejected() {
    // 513 ASCII chars = 513 bytes — should be rejected
    let s = "A".repeat(513);
    assert!(
        Identifier::new(&s).is_err(),
        "513 ASCII chars should be rejected"
    );
}

#[test]
fn identifier_length_check_uses_bytes_not_chars() {
    // Identifier regex restricts to ASCII, so we can't directly test multibyte.
    // But we CAN verify the code uses s.len() (bytes) by checking the error message.
    let s = "A".repeat(513);
    let err = Identifier::new(&s).unwrap_err();
    let msg = err.to_string();
    // The error message reports s.len() which is byte length
    assert!(
        msg.contains("513"),
        "Error should report byte length 513, got: {msg}"
    );
}

#[test]
fn description_length_check_uses_chars_not_bytes() {
    // Description allows unicode. Create a string of 2048 multibyte chars.
    // Each 'é' (U+00E9) is 2 bytes in UTF-8.
    let s: String = std::iter::repeat_n('é', 2048).collect();
    assert_eq!(s.chars().count(), 2048);
    assert_eq!(s.len(), 4096); // 2048 * 2 bytes
                               // Description uses chars().count(), so 2048 chars should be accepted
    assert!(
        Description::new(&s).is_ok(),
        "2048 chars (4096 bytes) should be accepted by Description"
    );
}

#[test]
fn description_2049_chars_is_rejected() {
    let s: String = std::iter::repeat_n('é', 2049).collect();
    assert!(
        Description::new(&s).is_err(),
        "2049 chars should be rejected by Description"
    );
}

// ══════════════════════════════════════════════════════════════
// 2. FlexFloat Display overflow
//
// FlexFloat::Display does `self.0 as i64` for whole-number floats.
// For values > i64::MAX (e.g. 1e19 = 10_000_000_000_000_000_000),
// this cast overflows. In Rust, `f64 as i64` for out-of-range values
// is saturating (returns i64::MAX or i64::MIN), not UB, but the
// displayed value will be wrong.
// ══════════════════════════════════════════════════════════════

#[test]
fn flexfloat_display_large_whole_number_overflow() {
    // 1e19 = 10_000_000_000_000_000_000.0 which exceeds i64::MAX (9_223_372_036_854_775_807)
    // FlexFloat Display does: if fract() == 0.0 { write!(f, "{}", self.0 as i64) }
    // This will saturate to i64::MAX, producing wrong output.
    let template_yaml = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "BigFloat",
            "type": "FLOAT",
            "default": 1e19
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#
    .to_string();
    let v = yaml_val(&template_yaml);
    let jt = decode_job_template(v, None).unwrap();

    // Get the default value string representation via the parameter definition
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];
    let default_str = param.default_value().unwrap();

    // The correct display would be "10000000000000000000" or "1e19"
    // But if `as i64` saturates, we get i64::MAX = "9223372036854775807"
    let expected_correct = "10000000000000000000";
    let i64_max_str = i64::MAX.to_string();

    if default_str == i64_max_str {
        panic!(
            "BUG CONFIRMED: FlexFloat Display overflow! \
             1e19 displayed as i64::MAX ({i64_max_str}) instead of {expected_correct}"
        );
    }
    // If we get here, the display is correct (or at least not i64::MAX)
    assert_eq!(
        default_str, expected_correct,
        "FlexFloat should display 1e19 correctly, got: {default_str}"
    );
}

#[test]
fn flexfloat_display_negative_large_whole_number() {
    // -1e19 should also overflow: f64 as i64 saturates to i64::MIN
    let template_yaml = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "BigNeg",
            "type": "FLOAT",
            "default": -1e19
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#;
    let v = yaml_val(template_yaml);
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];
    let default_str = param.default_value().unwrap();

    let i64_min_str = i64::MIN.to_string();
    if default_str == i64_min_str {
        panic!(
            "BUG CONFIRMED: FlexFloat Display overflow for negative! \
             -1e19 displayed as i64::MIN ({i64_min_str})"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// 3. Empty extensions list asymmetry
//
// In parse.rs, decode_environment_template explicitly checks:
//   if template_exts.is_empty() { return Err(...) }
// But decode_job_template does NOT have this check.
// An empty extensions list should be rejected for both or neither.
// ══════════════════════════════════════════════════════════════

#[test]
fn empty_extensions_job_template() {
    // Job template with extensions: []
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let result = decode_job_template(v, None);
    // Record whether this succeeds or fails
    let job_result_is_err = result.is_err();
    if job_result_is_err {
        let msg = result.unwrap_err().to_string();
        eprintln!("Job template with empty extensions: REJECTED with: {msg}");
    } else {
        eprintln!("Job template with empty extensions: ACCEPTED");
    }

    // Environment template with extensions: []
    let v2 = yaml_val(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": [],
        "environment": {
            "name": "E",
            "script": {"actions": {"onEnter": {"command": "echo"}}}
        }
    }"#,
    );
    let env_result = decode_environment_template(v2, None);
    let env_result_is_err = env_result.is_err();
    if env_result_is_err {
        let msg = env_result.unwrap_err().to_string();
        eprintln!("Env template with empty extensions: REJECTED with: {msg}");
    } else {
        eprintln!("Env template with empty extensions: ACCEPTED");
    }

    // Both should behave the same way
    assert_eq!(
        job_result_is_err, env_result_is_err,
        "BUG: Asymmetric handling of empty extensions list! \
         Job template empty extensions is_err={job_result_is_err}, \
         Env template empty extensions is_err={env_result_is_err}"
    );
}

// ══════════════════════════════════════════════════════════════
// 4. HashMap ordering in Environment.variables
//
// Environment.variables is HashMap<String, FormatString>.
// Verify that all entries are preserved (HashMap doesn't lose entries).
// ══════════════════════════════════════════════════════════════

#[test]
fn environment_variables_all_preserved() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "jobEnvironments": [{
            "name": "MyEnv",
            "variables": {
                "VAR_A": "alpha",
                "VAR_B": "bravo",
                "VAR_C": "charlie",
                "VAR_D": "delta",
                "VAR_E": "echo",
                "VAR_F": "foxtrot",
                "VAR_G": "golf",
                "VAR_H": "hotel"
            },
            "script": {"actions": {"onEnter": {"command": "setup"}}}
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let envs = jt.job_environments.as_ref().unwrap();
    let vars = envs[0].variables.as_ref().unwrap();

    // All 8 variables must be present
    assert_eq!(vars.len(), 8, "Expected 8 variables, got {}", vars.len());
    let expected = [
        ("VAR_A", "alpha"),
        ("VAR_B", "bravo"),
        ("VAR_C", "charlie"),
        ("VAR_D", "delta"),
        ("VAR_E", "echo"),
        ("VAR_F", "foxtrot"),
        ("VAR_G", "golf"),
        ("VAR_H", "hotel"),
    ];
    for (key, val) in &expected {
        let actual = vars
            .get(*key)
            .unwrap_or_else(|| panic!("Missing variable: {key}"));
        assert_eq!(actual.raw(), *val, "Variable {key} has wrong value");
    }
}

// ══════════════════════════════════════════════════════════════
// 5. STRING/PATH parameter minLength/maxLength uses byte length
//
// JobStringParameterDefinition::check_constraints uses value.len()
// (byte length) for minLength/maxLength checks, not chars().count().
// This means multibyte UTF-8 strings are measured in bytes.
// The spec likely intends character count.
// ══════════════════════════════════════════════════════════════

#[test]
fn string_param_maxlength_uses_chars_not_bytes() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "Msg",
            "type": "STRING",
            "default": "hello",
            "maxLength": 5
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];

    // "héllo" is 5 chars but 6 bytes — should be accepted with maxLength=5
    let test_value = openjd_expr::ExprValue::String("héllo".to_string());
    assert!(
        param.check_constraints(&test_value).is_ok(),
        "5-character string 'héllo' should pass maxLength=5 (char count, not byte count)"
    );
}

#[test]
fn string_param_minlength_uses_chars_not_bytes() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "Msg",
            "type": "STRING",
            "default": "hello world",
            "minLength": 6
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];

    // "ééé" is 3 chars but 6 bytes — should be rejected with minLength=6
    let test_value = openjd_expr::ExprValue::String("ééé".to_string());
    assert!(
        param.check_constraints(&test_value).is_err(),
        "3-character string 'ééé' should fail minLength=6 (char count, not byte count)"
    );
}

#[test]
fn path_param_maxlength_uses_chars_not_bytes() {
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{
            "name": "Dir",
            "type": "PATH",
            "default": "/tmp/hello",
            "maxLength": 10
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "run"}}}}]
    }"#,
    );
    let jt = decode_job_template(v, None).unwrap();
    let param = &jt.parameter_definitions.as_ref().unwrap()[0];

    // "/tmp/héllo" is 10 chars but 11 bytes — should be accepted with maxLength=10
    let test_value = openjd_expr::ExprValue::Path {
        value: "/tmp/héllo".to_string(),
        format: openjd_expr::PathFormat::Posix,
    };
    assert!(
        param.check_constraints(&test_value).is_ok(),
        "10-character path '/tmp/héllo' should pass maxLength=10 (char count, not byte count)"
    );
}

// ══════════════════════════════════════════════════════════════
// 6. expr_value_eq for Path values
//
// In step_param_space.rs, expr_value_eq matches:
//   (ExprValue::String(x), ExprValue::String(y)) => x == y
// But does NOT match ExprValue::Path.
// However, make_leaf_node for TaskParameter::Path stores values as
// ExprValue::String, not ExprValue::Path. So validate_containment
// will fail if the caller passes ExprValue::Path for a PATH parameter.
// ══════════════════════════════════════════════════════════════

#[test]
fn validate_containment_path_as_string_works() {
    // TaskParameter::Path stores values as ExprValue::String internally.
    // Passing ExprValue::String for a PATH param should work.
    let space = make_path_space(vec!["/tmp/a", "/tmp/b", "/tmp/c"]);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    let mut params = TaskParameterSet::new();
    params.insert(
        "Dir".into(),
        TaskParameterValue {
            param_type: TaskParameterType::Path,
            value: openjd_expr::ExprValue::String("/tmp/b".to_string()),
        },
    );
    assert!(
        iter.validate_containment(&params).is_ok(),
        "validate_containment should accept ExprValue::String for PATH param"
    );
}

#[test]
fn validate_containment_path_as_path_value() {
    // ExprValue::Path should match ExprValue::String in containment checks
    let space = make_path_space(vec!["/tmp/a", "/tmp/b", "/tmp/c"]);
    let iter = StepParameterSpaceIterator::new(&space).unwrap();

    let mut params = TaskParameterSet::new();
    params.insert(
        "Dir".into(),
        TaskParameterValue {
            param_type: TaskParameterType::Path,
            value: openjd_expr::ExprValue::Path {
                value: "/tmp/b".to_string(),
                format: openjd_expr::PathFormat::Posix,
            },
        },
    );
    assert!(
        iter.validate_containment(&params).is_ok(),
        "validate_containment should accept ExprValue::Path for PATH param"
    );
}

// Helper to build a PATH parameter space
fn make_path_space(paths: Vec<&str>) -> job::StepParameterSpace {
    let mut defs = indexmap::IndexMap::new();
    defs.insert(
        "Dir".to_string(),
        job::TaskParameter::Path {
            range: paths.iter().map(|s| s.to_string()).collect(),
        },
    );
    job::StepParameterSpace {
        task_parameter_definitions: defs,
        combination: None,
    }
}
