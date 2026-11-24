// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Gold standard error message tests.
//!
//! Every failure test asserts the full error output including:
//! - Error count and model name
//! - Field path (matching Python Pydantic format)
//! - Error message
//!
//! This ensures error messages are stable and match the Python implementation.

use openjd_model::CallerLimits;
use openjd_model::{decode_environment_template, decode_job_template};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_env_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_environment_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// Decode `yaml`, which must fail with exactly `expected`; `extensions`
/// are the ones the caller supports besides EXPR, FEATURE_BUNDLE_1 and
/// TASK_CHUNKING.
fn assert_message(extensions: &[&str], yaml: &str, expected: &str) {
    let mut supported = vec!["EXPR", "FEATURE_BUNDLE_1", "TASK_CHUNKING"];
    supported.extend(extensions);
    let err = decode_job_template(yaml_val(yaml), Some(&supported), &CallerLimits::default())
        .expect_err("Expected validation error");
    assert_eq!(err.to_string(), expected);
}

// ══════════════════════════════════════════════════════════════
// Template-level errors
// ══════════════════════════════════════════════════════════════

#[test]
fn empty_steps() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": []
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "JobTemplate: must have at least one step.",
        ],
    );
}

#[test]
fn empty_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["name:\n\tmust not be empty."],
    );
}

#[test]
fn empty_parameter_definitions() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions:\n\tif provided, must contain at least one element."],
    );
}

// ══════════════════════════════════════════════════════════════
// Parameter definition errors (with path)
// ══════════════════════════════════════════════════════════════

#[test]
fn duplicate_parameter_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {"name": "Foo", "type": "STRING"},
            {"name": "Foo", "type": "STRING"}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[1]:\n\tduplicate parameter name: 'Foo'"],
    );
}

/// A duplicated Job Parameter name is reported, and pass 8 types the name
/// by its last definition.
#[test]
fn a_duplicated_parameter_name_is_typed_by_its_last_definition() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: Test
parameterDefinitions: [{name: X, type: INT}, {name: X, type: STRING}]
steps:
- name: S
  script: {actions: {onRun: {command: "{{ Param.X + 1 }}"}}}
"#,
        "Model validation error: 2 validation errors for JobTemplate\n\
         parameterDefinitions[1]:\n\tduplicate parameter name: 'X'\n\
         steps[0] -> script -> actions -> onRun -> command:\n\tFailed to parse interpolation \
         expression at [0, 17]. Cannot use '+' operator with string and int\n  Param.X + 1\n  \
         ~~~~~~~~^~~",
    );
}

#[test]
fn int_default_above_max() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [
            {"name": "X", "type": "INT", "default": 100, "maxValue": 50}
        ],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["parameterDefinitions[0]:\n\t"],
    );
}

// ══════════════════════════════════════════════════════════════
// Step errors (with indexed path)
// ══════════════════════════════════════════════════════════════

#[test]
fn missing_script() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S"}]
    }"#,
        &["steps[0]:\n\tmust have 'script' or a simple action field."],
    );
}

#[test]
fn duplicate_step_name() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [
            {"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}},
            {"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}
        ]
    }"#,
        &["steps[1] -> name:\n\tduplicate step name: 'S'"],
    );
}

#[test]
fn empty_command() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": ""}}}}]
    }"#,
        &["steps[0] -> script -> actions -> onRun -> command:\n\tmust not be empty."],
    );
}

// ══════════════════════════════════════════════════════════════
// Host requirements errors (deeply nested path)
// ══════════════════════════════════════════════════════════════

#[test]
fn host_req_os_family_invalid() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "hostRequirements": {"attributes": [{"name": "attr.worker.os.family", "anyOf": ["ubuntu"]}]},
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "steps[0] -> hostRequirements -> attributes[0] -> anyOf:\n\t",
            "not valid for attr.worker.os.family",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Combination expression errors
// ══════════════════════════════════════════════════════════════

#[test]
fn combination_double_operator() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": [1]},
                    {"name": "B", "type": "INT", "range": [1]}
                ],
                "combination": "A ** B"
            },
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["steps[0] -> parameterSpace -> combination:\n\t"],
    );
}

#[test]
fn combination_duplicate_param() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "A", "type": "INT", "range": [1]}
                ],
                "combination": "A * A"
            },
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["steps[0] -> parameterSpace -> combination:\n\tparameter 'A' appears more than once"],
    );
}

// ══════════════════════════════════════════════════════════════
// Limit errors
// ══════════════════════════════════════════════════════════════

#[test]
fn job_name_too_long() {
    let long_name = "A".repeat(129);
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "{long_name}",
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "cmd"}}}}}}}}]
    }}"#
    );
    check_err(&s, &["name:\n\texceeds 128 characters."]);
}

// ══════════════════════════════════════════════════════════════
// Environment template errors
// ══════════════════════════════════════════════════════════════

#[test]
fn env_name_too_long() {
    let long_name = "A".repeat(65);
    let s = format!(
        r#"{{
        "specificationVersion": "environment-2023-09",
        "environment": {{
            "name": "{long_name}",
            "variables": {{"X": "1"}}
        }}
    }}"#
    );
    check_env_err(
        &s,
        &[
            "1 validation error for EnvironmentTemplate\n",
            "environment -> name:\n\texceeds 64 characters.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// EXPR extension errors
// ══════════════════════════════════════════════════════════════

#[test]
fn let_without_expr() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{"name": "S",
            "let": ["x = 1"],
            "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &["steps[0] -> let:\n\t'let' requires the EXPR extension."],
    );
}

#[test]
fn job_and_step_names_need_expr() {
    // An Environment action's `timeout` (FEATURE_BUNDLE_1) is in the job
    // scope, which has `Job.Name` only with EXPR.
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [FEATURE_BUNDLE_1]
name: Test
jobEnvironments:
- name: E
  script: {actions: {onEnter: {command: foo, timeout: "{{Job.Name}}"}}}
steps:
- name: S
  script: {actions: {onRun: {command: foo}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         jobEnvironments[0] -> script -> actions -> onEnter -> timeout:\n\tFailed to parse \
         interpolation expression at [0, 12]. Undefined variable: 'Job.Name'.\n  Job.Name\n  \
         ~~~~^~~~",
    );
    // A Step's template scope has `Step.Name` only with EXPR.
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
name: Test
steps:
- name: S
  parameterSpace: {taskParameterDefinitions: [{name: F, type: STRING, range: ["{{Step.Name}}"]}]}
  script: {actions: {onRun: {command: foo}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\tFailed to \
         parse interpolation expression at [0, 13]. Undefined variable: 'Step.Name'.\n  \
         Step.Name\n  ~~~~~^~~~",
    );
}

#[test]
fn complex_expr_in_environment_embedded_file_without_expr() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
name: Test
parameterDefinitions: [{name: X, type: INT}]
jobEnvironments:
- name: E
  script:
    embeddedFiles: [{name: f, type: TEXT, data: "{{ Param.X + 1 }}"}]
    actions: {onEnter: {command: foo}}
steps:
- name: S
  script: {actions: {onRun: {command: foo}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         jobEnvironments[0] -> script -> embeddedFiles[0] -> data:\n\t\
         complex expressions require the EXPR extension.",
    );
}

#[test]
fn a_chunked_task_parameter_is_a_range_expr() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR, TASK_CHUNKING]
name: Test
steps:
- name: S
  parameterSpace: {taskParameterDefinitions: [{name: F, type: "CHUNK[INT]", range: "1-4", chunks: {defaultTaskCount: 2, rangeConstraint: CONTIGUOUS}}]}
  script: {actions: {onRun: {command: "{{ Task.Param.F + 1 }}"}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         steps[0] -> script -> actions -> onRun -> command:\n\tFailed to parse interpolation \
         expression at [0, 22]. Cannot use '+' operator with range_expr and int\n  \
         Task.Param.F + 1\n  ~~~~~~~~~~~~~^~~",
    );
}

/// A list comprehension variable must not shadow a `let` binding of its
/// own script or, in a step script, of its Step.
#[test]
fn a_comprehension_variable_shadowing_a_let_binding() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: Test
steps:
- name: S
  let: ["x = 1"]
  script: {actions: {onRun: {command: "{{ [x for x in [1, 2]] }}"}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         steps[0] -> script -> actions -> onRun -> command:\n\t\
         List comprehension variable 'x' shadows a let binding",
    );
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: Test
steps:
- name: S
  stepEnvironments:
  - name: E
    script:
      let: ["x = 1"]
      actions: {onEnter: {command: "{{ [x for x in [1, 2]] }}"}}
  script: {actions: {onRun: {command: foo}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         JobTemplate: List comprehension variable 'x' shadows a let binding",
    );
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: Test
jobEnvironments:
- name: J
  script:
    let: ["x = 1"]
    actions: {onEnter: {command: "{{ [x for x in [1, 2]] }}"}}
steps:
- name: S
  script: {actions: {onRun: {command: foo}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         JobTemplate: List comprehension variable 'x' shadows a let binding",
    );
}

#[test]
fn complex_expr_without_expr() {
    check_err(r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "parameterDefinitions": [{"name": "X", "type": "INT", "default": 1}],
        "steps": [{"name": "S",
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.X + 1}}"]}}}}]
    }"#, &[
        "steps[0] -> script -> actions -> onRun -> args[0]:\n\tcomplex expressions require the EXPR extension.",
    ]);
}

// ══════════════════════════════════════════════════════════════
// Multiple errors in one template
// ══════════════════════════════════════════════════════════════

#[test]
fn multiple_errors() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "",
        "steps": []
    }"#,
        &[
            "2 validation errors for JobTemplate\n",
            "name:\n\tmust not be empty.",
            "JobTemplate: must have at least one step.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Extension list errors (Gold standard: full count + path + message)
//
// These tests pin the exact error output produced by the
// extensions-list validation pass in `parse.rs`. Matches the Python
// Pydantic wording for duplicates and unsupported names.
// ══════════════════════════════════════════════════════════════

#[test]
fn extensions_empty_list() {
    // An explicit empty `extensions: []` is rejected with a
    // single error at the `extensions` path.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": [],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tif provided, must be a non-empty list.",
        ],
    );
}

#[test]
fn extensions_single_unsupported() {
    // One unsupported extension produces one error with an
    // aggregated (single-value) "Unsupported extension names" message.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["NOT_A_REAL_EXTENSION"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: NOT_A_REAL_EXTENSION",
        ],
    );
}

#[test]
fn extensions_multiple_unsupported_sorted() {
    // Multiple unsupported extensions are reported in a single
    // message with names sorted alphabetically for stable output.
    // The input order here is deliberately reversed to verify
    // sort behavior.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["ZZZ_LAST", "AAA_FIRST", "MMM_MIDDLE"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tUnsupported extension names: AAA_FIRST, MMM_MIDDLE, ZZZ_LAST",
        ],
    );
}

#[test]
fn extensions_known_but_not_enabled_by_caller() {
    // A *recognized* ModelExtension (EXPR) still counts as
    // unsupported when the caller's allowlist excludes it — the
    // helper collapses both cases into one message, matching the
    // Python implementation. This test uses a restricted allowlist
    // (only FEATURE_BUNDLE_1) so that EXPR, though a known variant,
    // is not permitted here.
    let v = yaml_val(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
    );
    let err = decode_job_template(v, Some(&["FEATURE_BUNDLE_1"]), &CallerLimits::default())
        .expect_err("Expected validation error");
    let msg = err.to_string();
    for line in &[
        "1 validation error for JobTemplate\n",
        "extensions:\n\tUnsupported extension names: EXPR",
    ] {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

#[test]
fn extensions_single_duplicate() {
    // A single duplicate name is reported via a dedicated
    // "Duplicate values for extension name" message. The duplicate
    // name (EXPR) is also listed in the (empty here) "Unsupported"
    // pass set because EXPR *is* in the caller allowlist — so only
    // one error is produced.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: EXPR",
        ],
    );
}

#[test]
fn extensions_multiple_duplicates_sorted() {
    // Multiple duplicate names are listed comma-separated, sorted
    // alphabetically. Each duplicate is named at most once in the
    // output regardless of how many times it recurs in the input.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["TASK_CHUNKING", "EXPR", "TASK_CHUNKING", "EXPR", "FEATURE_BUNDLE_1"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "1 validation error for JobTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: EXPR,TASK_CHUNKING",
        ],
    );
}

#[test]
fn extensions_duplicate_and_unsupported_collected_together() {
    // Duplicates and unsupported names are independent passes —
    // when both apply, the caller sees both errors together. This
    // is the collect-all behavior: fail-fast would have reported
    // only the first. Order here is duplicate-pass before
    // unsupported-pass (they're added in that order).
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "EXPR", "NOT_A_REAL_EXTENSION"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "cmd"}}}}]
    }"#,
        &[
            "2 validation errors for JobTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: EXPR",
            "extensions:\n\tUnsupported extension names: NOT_A_REAL_EXTENSION",
        ],
    );
}

#[test]
fn extensions_errors_use_environment_template_model_name() {
    // Extension-list errors on an environment template use
    // `EnvironmentTemplate` as the model name in the count header.
    check_env_err(
        r#"{
        "specificationVersion": "environment-2023-09",
        "extensions": ["NOT_REAL", "NOT_REAL", "OTHER_BOGUS"],
        "environment": {"name": "E", "script": {"actions": {"onEnter": {"command": "cmd"}}}}
    }"#,
        &[
            "2 validation errors for EnvironmentTemplate\n",
            "extensions:\n\tDuplicate values for extension name are not allowed. Duplicate values: NOT_REAL",
            "extensions:\n\tUnsupported extension names: NOT_REAL, OTHER_BOGUS",
        ],
    );
}

// === EXPR: list membership type-checks the item against the element type ===

#[test]
fn membership_item_type_mismatch_is_refused_at_validation() {
    // openjd-specifications expr2.1.3--membership-element-type-mismatch:
    // `__contains__(list: list[T], item: T)` admits no signature for a
    // string item against list[int], so the template is refused at
    // validation instead of the expression evaluating to false.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {
            "command": "python", "args": ["-c", "print(r'{{ \"a\" in [1, 2] }}')"]}}}}]
    }"#,
        &[
            "steps[0] -> script -> actions -> onRun -> args[1]:",
            "Cannot use 'in' operator: item of type string is not compatible with the element type int of list[int]",
        ],
    );
}

#[test]
fn membership_unresolved_parameter_of_wrong_type_is_refused_at_validation() {
    // The static check reaches parameter references: a STRING parameter
    // tested against a list[int] can never match, so it fails at check
    // rather than on the worker.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "S", "type": "STRING", "default": "a"}],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {
            "command": "echo", "args": ["{{ Param.S in [1, 2] }}"]}}}}]
    }"#,
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "Cannot use 'in' operator: item of type string is not compatible with the element type int of list[int]",
        ],
    );
}

/// `RawParam.*` of a PATH is the unmapped string, and of a LIST[PATH] a
/// list of strings, so a path property is not available on either.
#[test]
fn raw_param_of_a_path_or_list_of_paths_is_a_string() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: "{{ RawParam.P.name }}"
parameterDefinitions:
- {name: P, type: PATH, default: a/b}
- {name: L, type: "LIST[PATH]", default: [a/b]}
steps:
- name: S
  script: {actions: {onRun: {command: "{{ RawParam.L[0].name }}", args: ["{{ RawParam.P.upper() }}", "{{ RawParam.L[0].upper() }}"]}}}
"#,
        "Model validation error: 2 validation errors for JobTemplate\n\
         name:\n\tFailed to parse interpolation expression at [0, 21]. 'name' property is not \
         available for string. Available for: path\n  RawParam.P.name\n  ~~~~~~~~~~~^~~~\n\
         steps[0] -> script -> actions -> onRun -> command:\n\tFailed to parse interpolation \
         expression at [0, 24]. 'name' property is not available for string. Available for: \
         path\n  RawParam.L[0].name\n  ~~~~~~~~~~~~~~^~~~",
    );
}

/// A Step Environment reads the values of the Step's `let` names from the
/// Step's task scope, after the script's own `let` bindings, so a script
/// `let` that shadows a Step `let` gives the Step Environment its type.
#[test]
fn a_step_environment_reads_step_let_names_after_the_script_let_bindings() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
steps:
- name: S
  let: ["x = 1"]
  script: {let: ["x = 'a'"], actions: {onRun: {command: "{{ x }}"}}}
  stepEnvironments:
  - {name: E, variables: {V: "{{ x + 1 }}"}}
"#,
        "Model validation error: 2 validation errors for JobTemplate\n\
         steps[0] -> script -> let[0]:\n\t'x' shadows enclosing scope.\n\
         steps[0] -> stepEnvironments[0] -> variables -> V:\n\tFailed to parse interpolation \
         expression at [0, 11]. Cannot use '+' operator with string and int\n  x + 1\n  ~~^~~",
    );
}

/// Without EXPR, a Step's `let` bindings bind nothing, so a Step `let`
/// named like a task-scope symbol does not bring that symbol into a Step
/// Environment.
#[test]
fn without_expr_a_step_let_adds_nothing_to_a_step_environment() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
name: T
steps:
- name: S
  let: ["Task.Param.T = 1"]
  parameterSpace: {taskParameterDefinitions: [{name: T, type: INT, range: "1-2"}]}
  script: {actions: {onRun: {command: "{{ Task.Param.T }}"}}}
  stepEnvironments:
  - {name: E, variables: {V: "{{ Task.Param.T }}"}}
"#,
        "Model validation error: 2 validation errors for JobTemplate\n\
         steps[0] -> let:\n\t'let' requires the EXPR extension.\n\
         steps[0] -> stepEnvironments[0] -> variables -> V:\n\tFailed to parse interpolation \
         expression at [0, 18]. Undefined variable: 'Task.Param.T'.\n  Task.Param.T\n  \
         ~~~~~~~~~~~^",
    );
}

/// Structure reports a literal amount name that repeats an earlier literal
/// one, and pass 8 reports a name that repeats a static one, so a literal
/// name that repeats both is reported by each.
#[test]
fn a_duplicate_amount_name_is_reported_by_structure_and_by_pass_8() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
steps:
- name: S
  hostRequirements:
    amounts:
    - {name: "{{ 'amount.custom.a' }}", min: 1}
    - {name: amount.custom.a, min: 1}
    - {name: amount.custom.a, min: 1}
  script: {actions: {onRun: {command: x}}}
"#,
        "Model validation error: 3 validation errors for JobTemplate\n\
         steps[0] -> hostRequirements -> amounts[2]:\n\tduplicate amount name 'amount.custom.a'.\n\
         steps[0] -> hostRequirements -> amounts[1]:\n\tduplicate amount name 'amount.custom.a'.\n\
         steps[0] -> hostRequirements -> amounts[2]:\n\tduplicate amount name 'amount.custom.a'.",
    );
}

/// Without EXPR, a Step Environment's `variables`, action `args` and
/// embedded-file `data` may use only bare references.
#[test]
fn without_expr_a_step_environment_rejects_complex_expressions() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
name: T
parameterDefinitions: [{name: S, type: STRING, default: a}]
steps:
- name: S
  script: {actions: {onRun: {command: x}}}
  stepEnvironments:
  - name: E
    variables: {V: "{{ Param.S + 'x' }}"}
    script:
      actions: {onEnter: {command: x, args: ["{{ Param.S + 'x' }}"]}}
      embeddedFiles: [{name: f, type: TEXT, data: "{{ Param.S + 'x' }}"}]
"#,
        "Model validation error: 3 validation errors for JobTemplate\n\
         steps[0] -> stepEnvironments[0] -> variables -> V:\n\t\
         complex expressions require the EXPR extension.\n\
         steps[0] -> stepEnvironments[0] -> script -> actions -> onEnter -> args[0]:\n\t\
         complex expressions require the EXPR extension.\n\
         steps[0] -> stepEnvironments[0] -> script -> embeddedFiles[0] -> data:\n\t\
         complex expressions require the EXPR extension.",
    );
}

/// A duplicated Job Parameter name is set by each definition in turn, and
/// the template scope has no PATH `Param.*`, so there an INT definition
/// followed by a PATH one types `Param.X` as an int.
#[test]
fn a_duplicated_parameter_name_keeps_its_non_path_type_in_the_template_scope() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: "{{ Param.X + 1 }}"
parameterDefinitions:
- {name: X, type: INT}
- {name: X, type: PATH}
steps:
- {name: S, script: {actions: {onRun: {command: x}}}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         parameterDefinitions[1]:\n\tduplicate parameter name: 'X'",
    );
}

/// The comprehension-variable rules apply only with EXPR, which a
/// comprehension needs.
#[test]
fn without_expr_no_comprehension_variable_rule_is_reported() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
name: T
steps:
- name: S
  let: ["x = 1"]
  script: {actions: {onRun: {command: "{{ [x for x in [1]] }}"}}}
"#,
        "Model validation error: 2 validation errors for JobTemplate\n\
         steps[0] -> let:\n\t'let' requires the EXPR extension.\n\
         steps[0] -> script -> actions -> onRun -> command:\n\t\
         complex expressions require the EXPR extension.",
    );
}

/// Each SimpleAction field gets its own copy of the task scope, so its
/// `let` bindings do not reach the Step Environments.
#[test]
fn a_simple_action_let_does_not_reach_the_step_environments() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR, FEATURE_BUNDLE_1]
name: T
steps:
- name: S
  let: ["x = 1"]
  bash: {script: "echo {{ x }}", let: ["x = 'a'"]}
  stepEnvironments:
  - {name: E, variables: {V: "{{ x + 1 }}"}}
"#,
        "Model validation error: 1 validation error for JobTemplate\n\
         steps[0] -> bash -> let[0]:\n\t'x' shadows enclosing scope.",
    );
}

/// A Step's task scope takes only the `let` bindings pass 8 evaluates, so a
/// binding with no expression named like a Job Parameter does not replace
/// the task scope's own value: with a duplicated name, the template scope's
/// INT `Param.Q` does not hide the task scope's PATH one.
#[test]
fn a_let_binding_with_no_expression_does_not_replace_a_task_scope_symbol() {
    assert_message(
        &[],
        r#"
specificationVersion: jobtemplate-2023-09
extensions: [EXPR]
name: T
parameterDefinitions:
- {name: Q, type: INT}
- {name: Q, type: PATH}
steps:
- name: S
  let: ["Param.Q ="]
  script: {actions: {onRun: {command: "{{ Param.Q + 1 }}"}}}
"#,
        "Model validation error: 3 validation errors for JobTemplate\n\
         parameterDefinitions[1]:\n\tduplicate parameter name: 'Q'\n\
         steps[0] -> let[0]:\n\tbinding 'Param.Q' has no expression after '='.\n\
         steps[0] -> script -> actions -> onRun -> command:\n\tFailed to parse interpolation \
         expression at [0, 17]. Cannot use '+' operator with path and int\n  Param.Q + 1\n  \
         ~~~~~~~~^~~",
    );
}
