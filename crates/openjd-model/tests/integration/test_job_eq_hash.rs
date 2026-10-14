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

use openjd_expr::value::Float64;
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

/// `Float64`'s constructors return `Result`; these keep the assertions readable.
fn f64_of(v: f64) -> Float64 {
    Float64::new(v).expect("finite")
}

fn f64_text(v: f64, text: &str) -> Float64 {
    Float64::with_str(v, text.to_string()).expect("finite")
}

#[test]
fn task_parameter_float_negative_zero_eq_and_hash() {
    let floats = |vals: &[f64]| TaskParameter::Float {
        range: vals.iter().map(|v| f64_of(*v)).collect(),
    };
    let a = floats(&[0.0, 1.5]);
    let b = floats(&[-0.0, 1.5]);
    assert_eq!(a, b, "-0.0 == 0.0 under PartialEq");
    assert_eq!(hash_of(&a), hash_of(&b), "hash must agree with eq");
    let c = floats(&[0.0, 2.5]);
    assert_ne!(a, c);
}

#[test]
fn task_parameter_float_rendering_text_participates_in_eq_and_hash() {
    // Same number, different rendering means a different command line. Hash must
    // follow eq, or a cache keyed on it could serve one for the other.
    let bare = TaskParameter::Float {
        range: vec![f64_of(2.5)],
    };
    let scaled = TaskParameter::Float {
        range: vec![f64_text(2.5, "2.50")],
    };
    assert_ne!(bare, scaled);
    assert_ne!(hash_of(&bare), hash_of(&scaled));

    let same = TaskParameter::Float {
        range: vec![f64_text(2.5, "2.50")],
    };
    assert_eq!(scaled, same);
    assert_eq!(hash_of(&scaled), hash_of(&same));

    // The converse: text that renders identically to no text at all is the same
    // element. eq compares the rendering, not the stored Option.
    let redundant = TaskParameter::Float {
        range: vec![f64_text(2.5, "2.5")],
    };
    assert_eq!(bare, redundant);
    assert_eq!(hash_of(&bare), hash_of(&redundant));
}

/// Zero has no sign, and `Float64::with_str` drops a signed zero's text when
/// rendering, so storing it would leave two elements that emit byte-identical
/// command lines comparing unequal and hashing differently.
#[test]
fn task_parameter_float_signed_zero_matches_its_rendering() {
    let floats = |v: f64, t: &str| TaskParameter::Float {
        range: vec![f64_text(v, t)],
    };
    let unsigned = floats(0.0, "0.0");
    let signed = floats(-0.0, "-0.0");
    assert_eq!(signed, unsigned, "both render 0.0");
    assert_eq!(hash_of(&signed), hash_of(&unsigned));

    // The decimal places survive the sign being dropped, so a signed and an
    // unsigned zero written to two places stay equal to each other and distinct
    // from one written to one place.
    assert_eq!(floats(-0.0, "-0.00"), floats(0.0, "0.00"));
    assert_ne!(floats(0.0, "0.00"), floats(0.0, "0.0"));

    // Zero-ness is decided from the text, not from the parse, which underflows in
    // both notations. An all-zero mantissa spells zero whatever the exponent; a
    // tiny value's digits are the author's request and survive.
    assert_eq!(f64_text(0.0, "-0e5").to_display_string(), "0e5");
    assert_eq!(f64_text(0.0, "1e-400").to_display_string(), "1e-400");
    let tiny = format!("0.{}1", "0".repeat(400));
    assert_eq!(f64_text(0.0, &tiny).to_display_string(), tiny);
}

/// `Float64` round-trips as the `<float> | <floatstring>` union, and routes through
/// its constructors so it cannot deserialize into a state they forbid.
///
/// Normalization and the length cap are deliberately *not* here. Those are §7.5
/// range-element rules applied by `resolve_float_range`, and `Float64` also carries
/// job parameter defaults and expression literals, which preserve their spelling
/// verbatim. `Deserialize` is not a validation boundary for any range type —
/// `TaskParameter::String`'s `Vec<String>` has no cap on this path either.
#[test]
fn float64_deserialize_routes_through_the_constructors() {
    let de = |json: &str| serde_json::from_str::<Float64>(json);

    // The union, both ways, exactly.
    assert_eq!(de("2.5").unwrap(), f64_of(2.5));
    assert_eq!(de(r#""2.50""#).unwrap(), f64_text(2.5, "2.50"));
    assert_eq!(serde_json::to_string(&f64_of(2.5)).unwrap(), "2.5");
    assert_eq!(
        serde_json::to_string(&f64_text(2.5, "2.50")).unwrap(),
        r#""2.50""#
    );

    // Surrounding whitespace cannot reach a command line.
    assert_eq!(de(r#"" 2.50 ""#).unwrap().to_display_string(), "2.50");

    // Non-finite is rejected rather than becoming a NaN `value`, which would stop
    // `PartialEq` being reflexive while `Hash` still agreed.
    assert!(de(r#""NaN""#).is_err());
    assert!(de(r#""inf""#).is_err());
    assert!(de(r#""-Infinity""#).is_err());

    // The constructors' zero rule applies here too: zero has no sign.
    assert_eq!(de(r#""-0.00""#).unwrap().to_display_string(), "0.00");
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
