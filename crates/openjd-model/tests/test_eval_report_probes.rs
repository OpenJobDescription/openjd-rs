// Exploratory tests for the model crate quality evaluation report.
// These probe potential bugs identified during code review.

use openjd_model::{create_job, decode_job_template};
use std::collections::HashMap;

fn yaml_val(s: &str) -> serde_yaml::Value {
    serde_yaml::from_str(s).unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// 1. BUG: validate_definition uses .len() (bytes) for STRING
//    allowedValues, but check_constraints uses .chars().count()
// ═══════════════════════════════════════════════════════════════════

#[test]
fn bug_string_param_allowed_values_byte_vs_char_length() {
    // "aéb" is 3 chars but 4 bytes. maxLength=3 should accept it
    // if using chars().count(), but validate_definition uses .len()
    // and reports length as 4.
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Greeting
            type: STRING
            maxLength: 3
            allowedValues: ["aéb"]
            default: "aéb"
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    // BUG: This should succeed (3 chars <= maxLength 3) but fails
    // because validate_definition uses byte length (4 > 3).
    assert!(result.is_err(), "Currently fails due to byte-length bug");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("length 4"),
        "Reports byte length 4 instead of char count 3: {}",
        msg
    );
}

// ═══════════════════════════════════════════════════════════════════
// 2. NaN/Infinity: accepted by FlexFloat (no explicit rejection)
//    serde_yaml 0.9 parses .nan/.inf and FlexFloat accepts them.
// ═══════════════════════════════════════════════════════════════════

#[test]
fn float_param_nan_accepted_by_flexfloat() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Val
            type: FLOAT
            default: .nan
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    // Document: NaN is currently accepted. No explicit rejection in FlexFloat.
    assert!(
        result.is_ok(),
        "NaN is accepted — no explicit rejection in FlexFloat"
    );
}

#[test]
fn float_param_infinity_accepted_by_flexfloat() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        parameterDefinitions:
          - name: Val
            type: FLOAT
            default: .inf
        steps:
          - name: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "Infinity is accepted — no explicit rejection in FlexFloat"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 3. Step names are plain String, not Identifier — Unicode accepted
// ═══════════════════════════════════════════════════════════════════

#[test]
fn step_name_accepts_unicode() {
    // StepTemplate.name is String, not Identifier. Unicode is valid.
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: "Stëp"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_ok(),
        "Step names accept Unicode (they are String, not Identifier)"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 4. Combination expression edge cases
// ═══════════════════════════════════════════════════════════════════

#[test]
fn combination_expr_empty_parens_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            parameterSpace:
              taskParameterDefinitions:
                - name: A
                  type: INT
                  range: [1, 2]
              combination: "()"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_err(),
        "Empty parentheses in combination should be rejected"
    );
}

#[test]
fn combination_expr_leading_star_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            parameterSpace:
              taskParameterDefinitions:
                - name: A
                  type: INT
                  range: [1, 2]
                - name: B
                  type: INT
                  range: [3, 4]
              combination: "* A * B"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_err(),
        "Leading star in combination should be rejected"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 5. Zero-dimension parameter space iteration
// ═══════════════════════════════════════════════════════════════════

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
    let jt = decode_job_template(template, None).unwrap();
    let params: HashMap<String, openjd_model::JobParameterValue> = HashMap::new();
    let job = create_job(&jt, &params).unwrap();
    let step = &job.steps[0];
    if let Some(ref space) = step.parameter_space {
        let iter = openjd_model::StepParameterSpaceIterator::new(space).unwrap();
        assert_eq!(iter.len(), 1);
    }
}

// ═══════════════════════════════════════════════════════════════════
// 6. Lazy parameter space with range expression (within 1024 limit)
// ═══════════════════════════════════════════════════════════════════

#[test]
fn lazy_param_space_range_expr_within_limit() {
    // max_task_param_range_len is 1024 for all configs (not raised by FB1)
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            parameterSpace:
              taskParameterDefinitions:
                - name: Frame
                  type: INT
                  range: "1-1024"
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let jt = decode_job_template(template, None).unwrap();
    let params: HashMap<String, openjd_model::JobParameterValue> = HashMap::new();
    let job = create_job(&jt, &params).unwrap();
    let space = job.steps[0].parameter_space.as_ref().unwrap();
    let iter = openjd_model::StepParameterSpaceIterator::new(space).unwrap();
    assert_eq!(iter.len(), 1024);
    let task = iter.get(1023).unwrap();
    assert_eq!(task.len(), 1);
}

// ═══════════════════════════════════════════════════════════════════
// 7. Duplicate environment names across job and step
// ═══════════════════════════════════════════════════════════════════

#[test]
fn duplicate_env_name_across_job_and_step_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        jobEnvironments:
          - name: SharedEnv
            variables:
              FOO: bar
        steps:
          - name: Step1
            stepEnvironments:
              - name: SharedEnv
                variables:
                  BAZ: qux
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_err(),
        "Duplicate env name across job and step should be rejected"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 8. Self-referencing step dependency
// ═══════════════════════════════════════════════════════════════════

#[test]
fn self_referencing_step_dependency_rejected() {
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        steps:
          - name: Step1
            dependencies:
              - dependsOn: Step1
            script:
              actions:
                onRun:
                  command: echo
    "#,
    );
    let result = decode_job_template(template, None);
    assert!(
        result.is_err(),
        "Self-referencing step dependency should be rejected"
    );
}

// ═══════════════════════════════════════════════════════════════════
// 9. SimpleAction with malformed format string
// ═══════════════════════════════════════════════════════════════════

#[test]
fn simple_action_malformed_format_string_behavior() {
    let exts = &["FEATURE_BUNDLE_1"];
    let template = yaml_val(
        r#"
        specificationVersion: "jobtemplate-2023-09"
        name: Test
        extensions:
          - FEATURE_BUNDLE_1
        steps:
          - name: Step1
            bash: "echo '{{broken'"
    "#,
    );
    let result = decode_job_template(template, Some(exts));
    // Validation catches the malformed format string before
    // resolve_syntax_sugar runs, so the silent fallback is not reached.
    // The validation error is about the format string parse failure.
    if let Ok(jt) = result {
        let step = &jt.steps[0];
        let script = step.resolve_syntax_sugar().unwrap();
        let embedded = &script.embedded_files.as_ref().unwrap()[0];
        let data = embedded.data.as_ref().unwrap();
        if data.raw().is_empty() {
            println!(
                "WARNING: SimpleAction script with malformed format string \
                 was silently replaced with empty string"
            );
        }
    }
    // Document: validation catches this, so the silent fallback is not exercised
    // in the normal decode path. But resolve_syntax_sugar() is public and could
    // be called on an unvalidated template.
}
