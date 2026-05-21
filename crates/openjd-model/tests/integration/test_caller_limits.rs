// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for CallerLimits — caller-imposed limits on job templates.

use openjd_model::{
    create_job, decode_job_template, preprocess_job_parameters, CallerLimits, PathParameterOptions,
};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn test_tmp_dir() -> &'static str {
    if cfg!(windows) {
        "C:\\tmp"
    } else {
        "/tmp"
    }
}

fn minimal_template(steps: usize) -> String {
    let steps_json: Vec<String> = (0..steps)
        .map(|i| {
            format!(
                r#"{{"name": "step{i}", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}"#
            )
        })
        .collect();
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{}]
    }}"#,
        steps_json.join(", ")
    )
}

fn template_with_envs(job_envs: usize, step_envs: usize) -> String {
    let job_env_list: Vec<String> = (0..job_envs)
        .map(|i| {
            format!(
                r#"{{"name": "JobEnv{i}", "script": {{"actions": {{"onEnter": {{"command": "echo"}}}}}}}}"#
            )
        })
        .collect();
    let step_env_list: Vec<String> = (0..step_envs)
        .map(|i| {
            format!(
                r#"{{"name": "StepEnv{i}", "script": {{"actions": {{"onEnter": {{"command": "echo"}}}}}}}}"#
            )
        })
        .collect();
    let step_envs_field = if step_envs > 0 {
        format!(r#", "stepEnvironments": [{}]"#, step_env_list.join(", "))
    } else {
        String::new()
    };
    let job_envs_field = if job_envs > 0 {
        format!(r#", "jobEnvironments": [{}]"#, job_env_list.join(", "))
    } else {
        String::new()
    };
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}{step_envs_field}}}]
        {job_envs_field}
    }}"#
    )
}

fn template_with_param_space(range_size: usize) -> String {
    let range: Vec<String> = (0..range_size).map(|i| i.to_string()).collect();
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{{
            "name": "S",
            "parameterSpace": {{
                "taskParameterDefinitions": [
                    {{"name": "Frame", "type": "INT", "range": [{}]}}
                ]
            }},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#,
        range.join(", ")
    )
}

fn template_with_two_steps_param_spaces(range1: usize, range2: usize) -> String {
    let r1: Vec<String> = (0..range1).map(|i| i.to_string()).collect();
    let r2: Vec<String> = (0..range2).map(|i| i.to_string()).collect();
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {{
                "name": "S1",
                "parameterSpace": {{
                    "taskParameterDefinitions": [
                        {{"name": "Frame", "type": "INT", "range": [{}]}}
                    ]
                }},
                "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
            }},
            {{
                "name": "S2",
                "parameterSpace": {{
                    "taskParameterDefinitions": [
                        {{"name": "Frame", "type": "INT", "range": [{}]}}
                    ]
                }},
                "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
            }}
        ]
    }}"#,
        r1.join(", "),
        r2.join(", ")
    )
}

// ══════════════════════════════════════════════════════════════
// max_step_count
// ══════════════════════════════════════════════════════════════

#[test]
fn max_step_count_within_limit() {
    let limits = CallerLimits {
        max_step_count: Some(3),
        ..Default::default()
    };
    let v = yaml_val(&minimal_template(3));
    let result = decode_job_template(v, None, &limits);
    assert!(
        result.is_ok(),
        "3 steps within limit of 3: {:?}",
        result.err()
    );
}

#[test]
fn max_step_count_exceeded() {
    let limits = CallerLimits {
        max_step_count: Some(2),
        ..Default::default()
    };
    let v = yaml_val(&minimal_template(3));
    let err = decode_job_template(v, None, &limits).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("exceeds caller limit of 2 steps"),
        "Expected step count error, got: {msg}"
    );
}

#[test]
fn max_step_count_none_means_no_limit() {
    let limits = CallerLimits::default();
    let v = yaml_val(&minimal_template(5));
    let result = decode_job_template(v, None, &limits);
    assert!(result.is_ok(), "No limit set: {:?}", result.err());
}

#[test]
fn decode_job_template_without_limits_still_works() {
    let v = yaml_val(&minimal_template(5));
    let result = decode_job_template(v, None, &CallerLimits::default());
    assert!(
        result.is_ok(),
        "Original API still works: {:?}",
        result.err()
    );
}

// ══════════════════════════════════════════════════════════════
// max_env_count
// ══════════════════════════════════════════════════════════════

#[test]
fn max_env_count_within_limit() {
    let limits = CallerLimits {
        max_env_count: Some(5),
        ..Default::default()
    };
    // 2 job envs + 2 step envs = 4 total
    let v = yaml_val(&template_with_envs(2, 2));
    let result = decode_job_template(v, None, &limits);
    assert!(
        result.is_ok(),
        "4 envs within limit of 5: {:?}",
        result.err()
    );
}

#[test]
fn max_env_count_exceeded() {
    let limits = CallerLimits {
        max_env_count: Some(3),
        ..Default::default()
    };
    // 2 job envs + 2 step envs = 4 total
    let v = yaml_val(&template_with_envs(2, 2));
    let err = decode_job_template(v, None, &limits).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("total environments (4) exceeds caller limit of 3"),
        "Expected env count error, got: {msg}"
    );
}

#[test]
fn max_env_count_counts_job_and_step_envs_together() {
    let limits = CallerLimits {
        max_env_count: Some(4),
        ..Default::default()
    };
    // Exactly 4 total (2 + 2) — should pass
    let v = yaml_val(&template_with_envs(2, 2));
    let result = decode_job_template(v, None, &limits);
    assert!(result.is_ok(), "Exactly at limit: {:?}", result.err());
}

// ══════════════════════════════════════════════════════════════
// max_task_count (checked in create_job)
// ══════════════════════════════════════════════════════════════

#[test]
fn max_task_count_within_limit() {
    let limits = CallerLimits {
        max_task_count: Some(100),
        ..Default::default()
    };
    let v = yaml_val(&template_with_param_space(50));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let result = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    );
    assert!(
        result.is_ok(),
        "50 tasks within limit of 100: {:?}",
        result.err()
    );
}

#[test]
fn max_task_count_exceeded_single_step() {
    let limits = CallerLimits {
        max_task_count: Some(10),
        ..Default::default()
    };
    let v = yaml_val(&template_with_param_space(20));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let err = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Total task count (20) exceeds caller limit of 10"),
        "Expected task count error, got: {msg}"
    );
}

#[test]
fn max_task_count_exceeded_across_steps() {
    let limits = CallerLimits {
        max_task_count: Some(15),
        ..Default::default()
    };
    // 10 + 10 = 20 total tasks
    let v = yaml_val(&template_with_two_steps_param_spaces(10, 10));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let err = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Total task count (20) exceeds caller limit of 15"),
        "Expected task count error, got: {msg}"
    );
}

#[test]
fn max_task_count_none_means_no_limit() {
    let limits = CallerLimits::default();
    let v = yaml_val(&template_with_param_space(100));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let result = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    );
    assert!(result.is_ok(), "No limit set: {:?}", result.err());
}

#[test]
fn max_task_count_step_without_param_space_counts_as_one() {
    let limits = CallerLimits {
        max_task_count: Some(5),
        ..Default::default()
    };
    // 3 steps with no parameter space = 3 tasks total
    let v = yaml_val(&minimal_template(3));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let result = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    );
    assert!(
        result.is_ok(),
        "3 tasks within limit of 5: {:?}",
        result.err()
    );
}

// ══════════════════════════════════════════════════════════════
// max_task_count counts actual tasks, not chunks
// ══════════════════════════════════════════════════════════════

#[test]
fn max_task_count_counts_tasks_not_chunks() {
    // 100 tasks chunked into 1 chunk (defaultTaskCount=100).
    // The iterator would report len()=1 with default chunking,
    // but the actual task count is 100 and should exceed a limit of 50.
    let limits = CallerLimits {
        max_task_count: Some(50),
        ..Default::default()
    };
    let range: Vec<String> = (0..100).map(|i| i.to_string()).collect();
    let tmpl = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["TASK_CHUNKING"],
        "steps": [{{
            "name": "S",
            "parameterSpace": {{
                "taskParameterDefinitions": [
                    {{"name": "Frame", "type": "CHUNK[INT]", "range": [{}],
                      "chunks": {{"defaultTaskCount": 100, "rangeConstraint": "CONTIGUOUS"}}}}
                ]
            }},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#,
        range.join(", ")
    );
    let v = yaml_val(&tmpl);
    let jt = decode_job_template(v, Some(&["TASK_CHUNKING"]), &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let err = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Total task count (100) exceeds caller limit of 50"),
        "Should count 100 tasks, not 1 chunk. Got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// max_template_size (checked in document_string_to_object)
// ══════════════════════════════════════════════════════════════

#[test]
fn max_template_size_within_limit() {
    use openjd_model::template::parse::{document_string_to_object, DocumentType};
    let doc = r#"{"specificationVersion": "jobtemplate-2023-09", "name": "T", "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]}"#;
    let limits = CallerLimits {
        max_template_size: Some(10000),
        ..Default::default()
    };
    let result = document_string_to_object(doc, DocumentType::Json, &limits);
    assert!(result.is_ok(), "Within limit: {:?}", result.err());
}

#[test]
fn max_template_size_exceeded() {
    use openjd_model::template::parse::{document_string_to_object, DocumentType};
    let doc = r#"{"specificationVersion": "jobtemplate-2023-09", "name": "T", "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "echo"}}}}]}"#;
    let limits = CallerLimits {
        max_template_size: Some(10),
        ..Default::default()
    };
    let err = document_string_to_object(doc, DocumentType::Json, &limits).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("exceeds caller limit of 10 bytes"),
        "Expected template size error, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// max_step_script_size (checked in create_job)
// ══════════════════════════════════════════════════════════════

#[test]
fn max_step_script_size_within_limit() {
    let limits = CallerLimits {
        max_step_script_size: Some(100_000),
        ..Default::default()
    };
    let v = yaml_val(&minimal_template(1));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let result = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    );
    assert!(result.is_ok(), "Within limit: {:?}", result.err());
}

#[test]
fn max_step_script_size_exceeded() {
    let limits = CallerLimits {
        max_step_script_size: Some(1), // impossibly small
        ..Default::default()
    };
    let v = yaml_val(&minimal_template(1));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let err = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("script size") && msg.contains("exceeds caller limit of 1 bytes"),
        "Expected step script size error, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// max_environment_size (checked in create_job)
// ══════════════════════════════════════════════════════════════

#[test]
fn max_environment_size_within_limit() {
    let limits = CallerLimits {
        max_environment_size: Some(100_000),
        ..Default::default()
    };
    let v = yaml_val(&template_with_envs(1, 0));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let result = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    );
    assert!(result.is_ok(), "Within limit: {:?}", result.err());
}

#[test]
fn max_environment_size_exceeded() {
    let limits = CallerLimits {
        max_environment_size: Some(1), // impossibly small
        ..Default::default()
    };
    let v = yaml_val(&template_with_envs(1, 0));
    let jt = decode_job_template(v, None, &CallerLimits::default()).unwrap();
    let params = preprocess_job_parameters(
        &jt,
        &Default::default(),
        &[],
        &PathParameterOptions::new(test_tmp_dir(), test_tmp_dir()),
    )
    .unwrap();
    let err = create_job(
        &jt,
        &params,
        &jt.default_validation_context()
            .with_caller_limits(limits.clone()),
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Environment") && msg.contains("exceeds caller limit of 1 bytes"),
        "Expected environment size error, got: {msg}"
    );
}

// ══════════════════════════════════════════════════════════════
// CallerLimits defaults
// ══════════════════════════════════════════════════════════════

#[test]
fn caller_limits_default_is_all_none() {
    let limits = CallerLimits::default();
    assert!(limits.max_step_count.is_none());
    assert!(limits.max_env_count.is_none());
    assert!(limits.max_task_count.is_none());
    assert!(limits.max_step_script_size.is_none());
    assert!(limits.max_environment_size.is_none());
    assert!(limits.max_template_size.is_none());
}

// ══════════════════════════════════════════════════════════════
// Combined limits
// ══════════════════════════════════════════════════════════════

#[test]
fn multiple_caller_limits_all_checked() {
    let limits = CallerLimits {
        max_step_count: Some(1),
        max_env_count: Some(0),
        ..Default::default()
    };
    // 2 steps + 1 env — both limits exceeded
    let v = yaml_val(&template_with_envs(1, 0));
    // This template has 1 step and 1 job env — step count is fine (1), but env count (1) > 0
    let err = decode_job_template(v, None, &limits).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("total environments (1) exceeds caller limit of 0"),
        "Expected env count error, got: {msg}"
    );
}
