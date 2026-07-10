// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests for `PartialEq` and `Hash` on the instantiated job types
//! (`job::Job`, `job::Step`, and friends).
//!
//! Invariant under test: `a == b ⇒ hash(a) == hash(b)`, including the
//! float edge cases (-0.0 vs 0.0) and map insertion-order independence.
//! Equality is on the *created job*: `resolved_symtab` participates, and
//! its transport format preserves original float literals, so jobs
//! created from `1.0` vs `1.00` parameter values compare unequal.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use openjd_expr::{ExprValue, FormatString};
use openjd_model::job::{
    AmountRequirement, Environment, HostRequirements, Job, StepParameterSpace, TaskParameter,
};
use openjd_model::CallerLimits;
use openjd_model::{create_job, decode_job_template, JobParameterInputValues};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

/// Preprocess parameters against a throwaway temp dir (none of the
/// templates here use PATH parameters, so the dirs are inert).
fn preprocess(
    jt: &openjd_model::template::JobTemplate,
    input: &JobParameterInputValues,
) -> openjd_model::JobParameterValues {
    let td = tempfile::TempDir::new().unwrap();
    let dir = td.path().to_str().unwrap();
    openjd_model::preprocess_job_parameters(
        jt,
        input,
        &[],
        &openjd_model::PathParameterOptions::new(dir, dir),
    )
    .unwrap()
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Create a job from the template JSON with the given FLOAT parameter value.
fn job_with_float_param(float_literal: &str) -> Job {
    let template = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "TestJob",
        "parameterDefinitions": [
            {"name": "F", "type": "FLOAT", "default": 0.5}
        ],
        "steps": [{
            "name": "S",
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.F}}"]}}}
        }]
    }"#,
    );
    let jt = decode_job_template(template, None, &CallerLimits::default()).unwrap();
    let mut input: JobParameterInputValues = HashMap::new();
    input.insert("F".to_string(), ExprValue::String(float_literal.into()));
    let params = preprocess(&jt, &input);
    create_job(&jt, &params, &jt.default_validation_context()).unwrap()
}

fn full_job() -> Job {
    let template = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "FullJob",
        "parameterDefinitions": [
            {"name": "N", "type": "INT", "default": 3}
        ],
        "jobEnvironments": [{
            "name": "JobEnv",
            "variables": {"A": "alpha", "B": "bravo", "C": "charlie"}
        }],
        "steps": [
            {
                "name": "First",
                "parameterSpace": {
                    "taskParameterDefinitions": [
                        {"name": "Frame", "type": "INT", "range": "1-10"},
                        {"name": "Weight", "type": "FLOAT", "range": [1.5, 2.5]}
                    ]
                },
                "hostRequirements": {
                    "amounts": [{"name": "amount.slots", "min": 2, "max": 8.5}],
                    "attributes": [{"name": "attr.os", "anyOf": ["linux"]}]
                },
                "script": {"actions": {"onRun": {"command": "run", "args": ["{{Task.Param.Frame}}"]}}}
            },
            {
                "name": "Second",
                "dependencies": [{"dependsOn": "First"}],
                "script": {
                    "embeddedFiles": [{"name": "f", "type": "TEXT", "data": "hi"}],
                    "actions": {"onRun": {
                        "command": "run2",
                        "timeout": "60",
                        "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "5"}
                    }}
                }
            }
        ]
    }"#,
    );
    let jt = decode_job_template(template, None, &CallerLimits::default()).unwrap();
    let params = preprocess(&jt, &Default::default());
    create_job(&jt, &params, &jt.default_validation_context()).unwrap()
}

// ══════════════════════════════════════════════════════════════
// Whole-job equality + hashing
// ══════════════════════════════════════════════════════════════

#[test]
fn identical_jobs_equal_and_hash_equal() {
    let a = full_job();
    let b = full_job();
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
    // Field-level spot checks on the pieces exercised by the template.
    assert_eq!(a.steps, b.steps);
    assert_eq!(a.parameters, b.parameters);
    assert_eq!(a.job_environments, b.job_environments);
    assert_eq!(hash_of(&a.steps[0]), hash_of(&b.steps[0]));
    assert_eq!(hash_of(&a.steps[1]), hash_of(&b.steps[1]));
}

#[test]
fn cloned_job_equal_and_hash_equal() {
    let a = full_job();
    let b = a.clone();
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn different_param_value_unequal() {
    let template = |n: i64| {
        let t = yaml_val(&format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "T",
            "parameterDefinitions": [{{"name": "N", "type": "INT", "default": {n}}}],
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo", "args": ["{{{{Param.N}}}}"]}}}}}}}}]
        }}"#
        ));
        let jt = decode_job_template(t, None, &CallerLimits::default()).unwrap();
        let params = preprocess(&jt, &Default::default());
        create_job(&jt, &params, &jt.default_validation_context()).unwrap()
    };
    let a = template(1);
    let b = template(2);
    assert_ne!(a, b);
    assert_ne!(a.steps[0], b.steps[0]);
}

#[test]
fn steps_usable_in_hash_set() {
    let a = full_job();
    let b = full_job();
    let mut set = std::collections::HashSet::new();
    // HashSet requires Eq; Step is PartialEq-only (f64 fields), so this
    // exercises Step's Hash via a set of hash values instead.
    set.insert(hash_of(&a.steps[0]));
    set.insert(hash_of(&b.steps[0]));
    set.insert(hash_of(&a.steps[1]));
    assert_eq!(set.len(), 2);
}

// ══════════════════════════════════════════════════════════════
// resolved_symtab participates in equality (equality on the
// created job, not the source template)
// ══════════════════════════════════════════════════════════════

#[test]
fn float_param_literal_participates_via_resolved_symtab() {
    let a = job_with_float_param("1.0");
    let b = job_with_float_param("1.00");
    // The bound ExprValues compare equal (value semantics ignore the
    // preserved literal)...
    assert_eq!(a.parameters["F"].value, b.parameters["F"].value);
    // ...but the step's resolved_symtab preserves the original literal in
    // transport form, so the created steps (and jobs) are unequal.
    assert_ne!(a.steps[0].resolved_symtab, b.steps[0].resolved_symtab);
    assert_ne!(a.steps[0], b.steps[0]);
    assert_ne!(a, b);
}

#[test]
fn same_float_literal_equal() {
    let a = job_with_float_param("1.25");
    let b = job_with_float_param("1.25");
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

// ══════════════════════════════════════════════════════════════
// Map-typed fields: insertion-order independence
// ══════════════════════════════════════════════════════════════

#[test]
fn environment_variables_order_insensitive_eq_and_hash() {
    let vars_ab: HashMap<String, FormatString> = [
        ("A".to_string(), FormatString::new("1").unwrap()),
        ("B".to_string(), FormatString::new("2").unwrap()),
    ]
    .into_iter()
    .collect();
    let vars_ba: HashMap<String, FormatString> = [
        ("B".to_string(), FormatString::new("2").unwrap()),
        ("A".to_string(), FormatString::new("1").unwrap()),
    ]
    .into_iter()
    .collect();
    let env = |vars: HashMap<String, FormatString>| Environment {
        name: "E".into(),
        description: None,
        script: None,
        variables: Some(vars),
        resolved_symtab: None,
    };
    let a = env(vars_ab);
    let b = env(vars_ba);
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn environment_no_variables_distinct_from_empty() {
    let env = |vars: Option<HashMap<String, FormatString>>| Environment {
        name: "E".into(),
        description: None,
        script: None,
        variables: vars,
        resolved_symtab: None,
    };
    let none = env(None);
    let empty = env(Some(HashMap::new()));
    assert_ne!(none, empty);
}

// ══════════════════════════════════════════════════════════════
// Float edge cases: -0.0 vs 0.0
// ══════════════════════════════════════════════════════════════

#[test]
fn amount_requirement_negative_zero_eq_and_hash() {
    let a = AmountRequirement {
        name: "amount.x".into(),
        min: Some(0.0),
        max: None,
    };
    let b = AmountRequirement {
        name: "amount.x".into(),
        min: Some(-0.0),
        max: None,
    };
    assert_eq!(a, b, "-0.0 == 0.0 under PartialEq");
    assert_eq!(hash_of(&a), hash_of(&b), "hash must agree with eq");
    let hr_a = HostRequirements {
        amounts: Some(vec![a]),
        attributes: None,
    };
    let hr_b = HostRequirements {
        amounts: Some(vec![b]),
        attributes: None,
    };
    assert_eq!(hr_a, hr_b);
    assert_eq!(hash_of(&hr_a), hash_of(&hr_b));
}

#[test]
fn task_parameter_float_negative_zero_eq_and_hash() {
    let a = TaskParameter::Float {
        range: vec![0.0, 1.5],
    };
    let b = TaskParameter::Float {
        range: vec![-0.0, 1.5],
    };
    assert_eq!(a, b, "-0.0 == 0.0 under PartialEq");
    assert_eq!(hash_of(&a), hash_of(&b), "hash must agree with eq");
    let c = TaskParameter::Float {
        range: vec![0.0, 2.5],
    };
    assert_ne!(a, c);
}

#[test]
fn step_parameter_space_order_insensitive_hash() {
    let p1 = (
        "A".to_string(),
        TaskParameter::String {
            range: vec!["x".into()],
        },
    );
    let p2 = (
        "B".to_string(),
        TaskParameter::String {
            range: vec!["y".into()],
        },
    );
    let a = StepParameterSpace {
        task_parameter_definitions: [p1.clone(), p2.clone()].into_iter().collect(),
        combination: None,
    };
    let b = StepParameterSpace {
        task_parameter_definitions: [p2, p1].into_iter().collect(),
        combination: None,
    };
    // IndexMap equality is order-insensitive, so hash must be too.
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}
