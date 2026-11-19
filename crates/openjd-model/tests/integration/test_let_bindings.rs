// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Tests ported from Python test/openjd/model/v2023_09/test_let_bindings.py
//!
//! Gold standard: failure tests assert the full error message including path.

use openjd_model::decode_job_template;
use openjd_model::CallerLimits;

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect("Expected success");
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err(&format!("Expected error for: {s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn job_with_step_let(let_bindings: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{{
            "name": "S",
            "let": [{let_bindings}],
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    )
}

fn job_with_script_let(let_bindings: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{{
            "name": "S",
            "script": {{
                "let": [{let_bindings}],
                "actions": {{"onRun": {{"command": "foo"}}}}
            }}
        }}]
    }}"#
    )
}

// === Valid let bindings ===

#[test]
fn test_let_simple() {
    decode_ok(&job_with_step_let(r#""x = 1""#));
}

#[test]
fn test_let_no_spaces() {
    decode_ok(&job_with_step_let(r#""x=1""#));
}

#[test]
fn test_let_complex_expr() {
    decode_ok(&job_with_step_let(r#""myVar = 1 + 2""#));
}

#[test]
fn test_let_underscore_prefix() {
    decode_ok(&job_with_step_let(r#""_private = 1""#));
}

#[test]
fn test_let_with_digit() {
    decode_ok(&job_with_step_let(r#""x2 = 1""#));
}

#[test]
fn test_let_multiple() {
    decode_ok(&job_with_step_let(r#""x = 1", "y = 2""#));
}

#[test]
fn test_let_in_script() {
    decode_ok(&job_with_script_let(r#""x = 1""#));
}

// === Invalid let bindings — step-level ===

#[test]
fn test_let_no_equals() {
    check_err(
        &job_with_step_let(r#""x""#),
        &["steps[0] -> let[0]:\n\tmissing '=' in 'x'."],
    );
}

#[test]
fn test_let_no_name() {
    check_err(
        &job_with_step_let(r#""= 1""#),
        &["steps[0] -> let[0]:\n\thas empty name."],
    );
}

#[test]
fn test_let_no_expression() {
    check_err(
        &job_with_step_let(r#""x =""#),
        &["steps[0] -> let[0]:\n\tbinding 'x' has no expression after '='."],
    );
}

#[test]
fn test_let_uppercase_start() {
    check_err(
        &job_with_step_let(r#""Param = 1""#),
        &["steps[0] -> let[0]:\n\tname 'Param' must start with lowercase letter or underscore."],
    );
}

#[test]
fn test_let_digit_start() {
    check_err(
        &job_with_step_let(r#""1x = 1""#),
        &["steps[0] -> let[0]:\n\tname '1x' must start with lowercase letter or underscore."],
    );
}

#[test]
fn test_let_duplicate_names() {
    check_err(
        &job_with_step_let(r#""x = 1", "x = 2""#),
        &["steps[0] -> let[1]:\n\tduplicate name 'x'."],
    );
}

#[test]
fn test_let_empty_step() {
    check_err(
        &job_with_step_let(""),
        &["steps[0] -> let:\n\tif provided, must not be empty."],
    );
}

#[test]
fn test_let_self_reference_step() {
    check_err(
        &job_with_step_let(r#""x = x + 1""#),
        &["steps[0] -> let[0]:\n\t'x' references itself."],
    );
}

#[test]
fn test_let_max_50_step() {
    let bindings: Vec<String> = (0..51).map(|i| format!(r#""x{i} = {i}""#)).collect();
    check_err(
        &job_with_step_let(&bindings.join(", ")),
        &["steps[0] -> let:\n\tmust not contain more than 50 bindings."],
    );
}

// === Invalid let bindings — script-level ===

#[test]
fn test_let_no_equals_script() {
    check_err(
        &job_with_script_let(r#""x""#),
        &["steps[0] -> script -> let[0]:\n\tmissing '=' in 'x'."],
    );
}

#[test]
fn test_let_no_name_script() {
    check_err(
        &job_with_script_let(r#""= 1""#),
        &["steps[0] -> script -> let[0]:\n\thas empty name."],
    );
}

#[test]
fn test_let_no_expression_script() {
    check_err(
        &job_with_script_let(r#""x =""#),
        &["steps[0] -> script -> let[0]:\n\tbinding 'x' has no expression after '='."],
    );
}

#[test]
fn test_let_uppercase_start_script() {
    check_err(&job_with_script_let(r#""Param = 1""#), &[
        "steps[0] -> script -> let[0]:\n\tname 'Param' must start with lowercase letter or underscore.",
    ]);
}

#[test]
fn test_let_digit_start_script() {
    check_err(&job_with_script_let(r#""1x = 1""#), &[
        "steps[0] -> script -> let[0]:\n\tname '1x' must start with lowercase letter or underscore.",
    ]);
}

#[test]
fn test_let_duplicate_names_script() {
    check_err(
        &job_with_script_let(r#""x = 1", "x = 2""#),
        &["steps[0] -> script -> let[1]:\n\tduplicate name 'x'."],
    );
}

#[test]
fn test_let_empty_script() {
    check_err(
        &job_with_script_let(""),
        &["steps[0] -> script -> let:\n\tif provided, must not be empty."],
    );
}

#[test]
fn test_let_self_reference_script() {
    check_err(
        &job_with_script_let(r#""x = x + 1""#),
        &["steps[0] -> script -> let[0]:\n\t'x' references itself."],
    );
}

#[test]
fn test_let_max_50_script() {
    let bindings: Vec<String> = (0..51).map(|i| format!(r#""x{i} = {i}""#)).collect();
    check_err(
        &job_with_script_let(&bindings.join(", ")),
        &["steps[0] -> script -> let:\n\tmust not contain more than 50 bindings."],
    );
}

#[test]
fn test_no_shadowing_same_block() {
    check_err(
        &job_with_script_let(r#""x = 32", "y = x * 5", "x = -1""#),
        &["steps[0] -> script -> let[2]:\n\tduplicate name 'x'."],
    );
}

// === Let bindings require EXPR extension ===

// === Let binding expression evaluation (Phase 1 type checking) ===

#[test]
fn test_let_syntax_error_step() {
    check_err(
        &job_with_step_let(r#""x = 1 +""#),
        &["steps[0] -> let[0]:\n\tInvalid expression in let binding 'x':"],
    );
}

#[test]
fn test_let_syntax_error_script() {
    check_err(
        &job_with_script_let(r#""x = 1 +""#),
        &["steps[0] -> script -> let[0]:\n\tInvalid expression in let binding 'x':"],
    );
}

#[test]
fn test_let_undefined_symbol() {
    check_err(
        &job_with_step_let(r#""x = undefined_var""#),
        &["steps[0] -> let[0]:\n\tInvalid expression in let binding 'x':"],
    );
}

#[test]
fn test_let_type_error_int_plus_string() {
    check_err(
        &job_with_step_let(r#""x = 1 + 'hello'""#),
        &["steps[0] -> let[0]:\n\tInvalid expression in let binding 'x':"],
    );
}

#[test]
fn test_let_type_propagation_to_later_binding() {
    // x is inferred as int, y = x + "hello" should fail because int + string is a type error
    check_err(
        &job_with_step_let(r#""x = 42", "y = x + 'hello'""#),
        &["steps[0] -> let[1]:\n\tInvalid expression in let binding 'y':"],
    );
}

#[test]
fn test_let_type_propagation_success() {
    // x is int, y = x + 1 should succeed (int + int = int)
    decode_ok(&job_with_step_let(r#""x = 42", "y = x + 1""#));
}

#[test]
fn test_let_type_propagation_across_scopes() {
    // Step-level x is int, script-level y = x + 1 should succeed
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["x = 42"],
            "script": {
                "let": ["y = x + 1"],
                "actions": {"onRun": {"command": "foo"}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_let_type_propagation_across_scopes_type_error() {
    // Step-level x is int, script-level y = x + "hello" should fail
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["x = 42"],
            "script": {
                "let": ["y = x + 'hello'"],
                "actions": {"onRun": {"command": "foo"}}
            }
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("steps[0] -> script -> let[0]:\n\tInvalid expression in let binding 'y':"),
        "Missing expected error.\nGot:\n{msg}"
    );
}

#[test]
fn test_let_with_param_reference() {
    // Let binding referencing a job parameter should type-check
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "Count", "type": "INT", "default": 5}],
        "steps": [{
            "name": "S",
            "let": ["doubled = Param.Count * 2"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{doubled}}"]}}}
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_let_param_type_mismatch() {
    // Param.Name is STRING, x = Param.Name + 1 should fail (string + int)
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "Name", "type": "STRING", "default": "hello"}],
        "steps": [{
            "name": "S",
            "let": ["x = Param.Name + 1"],
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("steps[0] -> let[0]:\n\tInvalid expression in let binding 'x':"),
        "Missing expected error.\nGot:\n{msg}"
    );
}

#[test]
fn test_let_inferred_type_used_in_format_string() {
    // x = 42 (int), then {{x + 1}} in command should work
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["x = 42"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{x + 1}}"]}}}
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_let_error_does_not_cascade() {
    // First binding has a syntax error, second binding should still be checked
    // (first gets ANY type so second doesn't cascade)
    check_err(
        &job_with_step_let(r#""x = 1 +", "y = 2""#),
        &["steps[0] -> let[0]:\n\tInvalid expression in let binding 'x':"],
    );
}

// === Let binding scope-appropriate library selection ===

#[test]
fn test_let_apply_path_mapping_rejected_in_step_scope() {
    // Step-level let bindings are TEMPLATE scope — apply_path_mapping is not available
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["mapped = apply_path_mapping('C:/foo')"],
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("steps[0] -> let[0]:\n\tInvalid expression in let binding 'mapped':"),
        "Step-level let should reject apply_path_mapping.\nGot:\n{msg}"
    );
}

#[test]
fn test_let_apply_path_mapping_accepted_in_script_scope() {
    // Script-level let bindings are TASK scope — apply_path_mapping is available
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["mapped = apply_path_mapping('C:/foo')"],
                "actions": {"onRun": {"command": "echo", "args": ["{{mapped}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_let_apply_path_mapping_accepted_in_env_scope() {
    // Environment script let bindings are SESSION scope — apply_path_mapping is available
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{
            "name": "E",
            "script": {
                "let": ["mapped = apply_path_mapping('C:/foo')"],
                "actions": {"onEnter": {"command": "echo", "args": ["{{mapped}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

// === Let bindings require EXPR extension ===

#[test]
fn test_let_without_expr_extension_step() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "let": ["x = 1"],
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(v, None, &CallerLimits::default()).expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("steps[0] -> let:\n\t'let' requires the EXPR extension."),
        "Missing expected error.\nGot:\n{msg}"
    );
}

#[test]
fn test_let_without_expr_extension_script() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {
                "let": ["x = 1"],
                "actions": {"onRun": {"command": "foo"}}
            }
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(v, None, &CallerLimits::default()).expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("steps[0] -> script -> let:\n\t'let' requires the EXPR extension."),
        "Missing expected error.\nGot:\n{msg}"
    );
}

// === Let bindings in environment scripts ===

#[test]
fn test_let_in_env_script() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{
            "name": "E",
            "script": {
                "let": ["x = 1"],
                "actions": {"onEnter": {"command": "foo"}}
            }
        }]
    }"#;
    decode_ok(s);
}

// === Step-level and script-level let bindings ===

#[test]
fn test_step_and_script_let() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["x = 1"],
            "script": {
                "let": ["y = 2"],
                "actions": {"onRun": {"command": "foo"}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_step_and_script_let_same_name_error() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["x = 1"],
            "script": {
                "let": ["x = 2"],
                "actions": {"onRun": {"command": "foo"}}
            }
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("steps[0] -> script -> let[0]:\n\t'x' shadows enclosing scope."),
        "Missing expected error.\nGot:\n{msg}"
    );
}

// === File reference types are PATH (not STRING) ===

#[test]
fn test_env_file_has_path_properties() {
    // Env.File.* is PATH type, so .parent should work in let bindings
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{
            "name": "E",
            "script": {
                "embeddedFiles": [{"name": "cfg", "type": "TEXT", "data": "hello"}],
                "let": ["p = Env.File.cfg.parent"],
                "actions": {"onEnter": {"command": "echo", "args": ["{{p}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_task_file_has_path_properties() {
    // Task.File.* is PATH type and IS in scope for script-level let bindings:
    // `filename` is a plain string (never an expression), so the runtime
    // allocates embedded file paths — defining Task.File.* — before `let`
    // evaluation, mirroring Env.File.* in environment scripts.
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {
                "embeddedFiles": [{"name": "run", "type": "TEXT", "data": "echo hi"}],
                "let": ["fileStem = Task.File.run.stem"],
                "actions": {"onRun": {"command": "echo", "args": ["{{fileStem}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_env_file_string_conversion_after_path_property() {
    // string(Env.File.cfg.parent) should work — .parent on PATH, then string()
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}],
        "jobEnvironments": [{
            "name": "E",
            "script": {
                "embeddedFiles": [{"name": "cfg", "type": "TEXT", "data": "hello"}],
                "let": ["has_parent = len(string(Env.File.cfg.parent)) > 0"],
                "actions": {"onEnter": {"command": "echo", "args": ["{{has_parent}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

// === Edit distance suggestions for undefined variables ===

#[test]
fn test_typo_in_param_reference_suggests_correction() {
    // Param.Frane is a typo for Param.Frame — should suggest it
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Frame", "type": "INT", "default": 1}],
        "steps": [{
            "name": "S",
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{Param.Frane}}"]}}}
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("Did you mean: Param.Frame"),
        "Expected 'Did you mean: Param.Frame' in:\n{msg}"
    );
}

#[test]
fn test_typo_in_let_binding_suggests_correction() {
    // Let binding references Param.Scen instead of Param.Scene
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "Scene", "type": "STRING", "default": "forest"}],
        "steps": [{
            "name": "S",
            "let": ["x = Param.Scen"],
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("Did you mean: Param.Scene"),
        "Expected 'Did you mean: Param.Scene' in:\n{msg}"
    );
}

// === Let binding with list comprehension ===

#[test]
fn test_let_list_comprehension() {
    decode_ok(&job_with_step_let(r#""x = [i for i in range(10)]""#));
}

// === Let binding with extra whitespace ===

#[test]
fn test_let_extra_spaces() {
    decode_ok(&job_with_step_let(r#""x  =  1""#));
}

// === Let binding with tabs ===

#[test]
fn test_let_tabs() {
    decode_ok(&job_with_step_let(r#""x\t=\t1""#));
}

// === Step-to-stepEnvironment shadowing detection ===

#[test]
fn test_no_shadowing_step_to_step_environment() {
    // A let binding in a stepEnvironment must not shadow a step-level let binding (§3.6).
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["x = 1"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{x}}"]}}},
            "stepEnvironments": [{
                "name": "E",
                "script": {
                    "let": ["x = 2"],
                    "actions": {"onEnter": {"command": "foo"}}
                }
            }]
        }]
    }"#;
    check_err(s, &["'x' shadows enclosing scope."]);
}

// === Multiple step environments can reference step binding ===

#[test]
fn test_multiple_step_envs_reference_step_binding() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["base = 50"],
            "stepEnvironments": [
                {
                    "name": "Env1",
                    "script": {
                        "let": ["val1 = base + 1"],
                        "actions": {"onEnter": {"command": "echo", "args": ["{{val1}}"]}}
                    }
                },
                {
                    "name": "Env2",
                    "script": {
                        "let": ["val2 = base + 2"],
                        "actions": {"onEnter": {"command": "echo", "args": ["{{val2}}"]}}
                    }
                }
            ],
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#;
    decode_ok(s);
}

// === Job environment let bindings ===

#[test]
fn test_job_environment_chained_let_bindings() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "jobEnvironments": [{
            "name": "TestEnv",
            "script": {
                "let": ["a = 10", "b = a * 2", "msg = 'Value: ' + string(b)"],
                "actions": {"onEnter": {"command": "echo", "args": ["{{msg}}"]}}
            }
        }],
        "steps": [{"name": "S", "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#;
    decode_ok(s);
}

// === Let bindings require EXPR in template extensions field ===

#[test]
fn test_let_rejected_without_extensions_field_even_if_supported() {
    // Template has no extensions field, but EXPR is in supported_extensions — should fail
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "steps": [{
            "name": "S",
            "let": ["x = 1"],
            "script": {"actions": {"onRun": {"command": "foo"}}}
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(&["EXPR"]), &CallerLimits::default())
        .expect_err("Expected error");
    let msg = err.to_string();
    assert!(msg.contains("EXPR"), "Expected 'EXPR' in:\n{msg}");
}

// === Host context symbols in script-level let bindings ===

#[test]
fn test_script_let_session_working_directory() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {
                "let": ["work_dir = Session.WorkingDirectory / 'output'"],
                "actions": {"onRun": {"command": "echo", "args": ["{{work_dir}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_script_let_task_param() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "Frame", "type": "INT", "range": "1-10"}]
            },
            "script": {
                "let": ["frame_str = string(Task.Param.Frame)"],
                "actions": {"onRun": {"command": "echo", "args": ["{{frame_str}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_script_let_session_has_path_mapping_rules() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {
                "let": ["has_rules = Session.HasPathMappingRules"],
                "actions": {"onRun": {"command": "echo", "args": ["{{has_rules}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

#[test]
fn test_script_let_type_error_with_session_symbol() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {
                "let": ["bad = Session.WorkingDirectory + 5"],
                "actions": {"onRun": {"command": "echo"}}
            }
        }]
    }"#;
    let v = yaml_val(s);
    let err = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("Cannot use '+' operator with path and int"),
        "Expected type error in:\n{msg}"
    );
}

#[test]
fn test_script_let_chained_with_session_symbol() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "SubDir", "type": "STRING", "default": "output"}],
        "steps": [{
            "name": "S",
            "script": {
                "let": [
                    "work_dir = Session.WorkingDirectory / Param.SubDir",
                    "log_file = work_dir / 'render.log'"
                ],
                "actions": {"onRun": {"command": "echo", "args": ["{{log_file}}"]}}
            }
        }]
    }"#;
    decode_ok(s);
}

// EnvironmentTemplate let-binding extension validation is covered in
// test_environment_template.rs (env_template_let_* tests).

// === Step-level let bindings must NOT have host-context symbols or functions ===

#[test]
fn step_let_rejects_path_param() {
    // Param.BasePath is a PATH type — not available in template scope.
    // Step-level let bindings are template scope, so this must fail.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "BasePath", "type": "PATH", "default": "/input/file.exr"}],
        "steps": [{
            "name": "S",
            "let": ["p = Param.BasePath"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{p}}"]}}}
        }]
    }"#,
        &["Undefined variable: 'Param.BasePath'"],
    );
}

#[test]
fn step_let_rejects_session_working_directory() {
    // Session.WorkingDirectory is host-context only.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["d = Session.WorkingDirectory"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{d}}"]}}}
        }]
    }"#,
        &["Undefined variable: 'Session.WorkingDirectory'"],
    );
}

#[test]
fn step_let_rejects_apply_path_mapping() {
    // apply_path_mapping() is a host-context function, not available in template scope.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "Input", "type": "STRING", "default": "/some/path"}],
        "steps": [{
            "name": "S",
            "let": ["mapped = apply_path_mapping(Param.Input)"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{mapped}}"]}}}
        }]
    }"#,
        &["apply_path_mapping"],
    );
}

#[test]
fn step_let_allows_non_path_params() {
    // Non-PATH params (INT, STRING, etc.) should work fine in step let bindings.
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {"name": "Count", "type": "INT", "default": 10},
            {"name": "Label", "type": "STRING", "default": "hello"}
        ],
        "steps": [{
            "name": "S",
            "let": ["doubled = Param.Count * 2", "msg = Param.Label + '_world'"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{doubled}} {{msg}}"]}}}
        }]
    }"#,
    );
}

#[test]
fn step_let_allows_raw_path_param_as_string() {
    // RawParam.BasePath is STRING type even for PATH params — available in template scope.
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "BasePath", "type": "PATH", "default": "/input/file.exr"}],
        "steps": [{
            "name": "S",
            "let": ["raw = RawParam.BasePath"],
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{raw}}"]}}}
        }]
    }"#,
    );
}

// === Script-level let bindings SHOULD have host-context symbols (as unresolved) ===

#[test]
fn script_let_allows_path_param_unresolved() {
    // Param.BasePath is available in script-level let bindings (host/session scope).
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "BasePath", "type": "PATH", "default": "/input/file.exr"}],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["p = Param.BasePath"],
                "actions": {"onRun": {"command": "echo", "args": ["{{p}}"]}}
            }
        }]
    }"#,
    );
}

#[test]
fn script_let_allows_session_working_directory() {
    // Session.WorkingDirectory is available in script-level let bindings.
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["d = Session.WorkingDirectory"],
                "actions": {"onRun": {"command": "echo", "args": ["{{d}}"]}}
            }
        }]
    }"#,
    );
}

#[test]
fn script_let_allows_apply_path_mapping() {
    // apply_path_mapping() is available in script-level let bindings (host context).
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [{"name": "Input", "type": "STRING", "default": "/some/path"}],
        "steps": [{
            "name": "S",
            "script": {
                "let": ["mapped = apply_path_mapping(Param.Input)"],
                "actions": {"onRun": {"command": "echo", "args": ["{{mapped}}"]}}
            }
        }]
    }"#,
    );
}

// === Identifier length (§3.6.1: <UserIdentifier> max 512 characters) ===
//
// Effective extensions are the intersection of the template's `extensions` list
// with the caller allowlist, so the FB1 cases need it declared in the body: the
// other helpers here emit `["EXPR"]` only.

fn job_with_step_let_fb1(let_bindings: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "steps": [{{
            "name": "S",
            "let": [{let_bindings}],
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    )
}

// FB1 off: the cap is 512, not the 64 that `max_identifier_len` would give.
#[test]
fn test_let_name_512_chars_succeeds() {
    let name = "a".repeat(512);
    decode_ok(&job_with_step_let(&format!(r#""{name} = 1""#)));
}

#[test]
fn test_let_name_513_chars_fails() {
    let name = "a".repeat(513);
    check_err(
        &job_with_step_let(&format!(r#""{name} = 1""#)),
        &[&format!(
            "steps[0] -> let[0]:\n\tname '{name}' exceeds 512 characters."
        )],
    );
}

// FB1 on: still 512, so the extension does not raise this cap either.
#[test]
fn test_let_name_512_chars_succeeds_with_fb1() {
    let name = "a".repeat(512);
    decode_ok(&job_with_step_let_fb1(&format!(r#""{name} = 1""#)));
}

#[test]
fn test_let_name_513_chars_fails_with_fb1() {
    let name = "a".repeat(513);
    check_err(
        &job_with_step_let_fb1(&format!(r#""{name} = 1""#)),
        &[&format!(
            "steps[0] -> let[0]:\n\tname '{name}' exceeds 512 characters."
        )],
    );
}

#[test]
fn test_let_name_512_chars_succeeds_script() {
    let name = "a".repeat(512);
    decode_ok(&job_with_script_let(&format!(r#""{name} = 1""#)));
}

#[test]
fn test_let_name_513_chars_fails_script() {
    let name = "a".repeat(513);
    check_err(
        &job_with_script_let(&format!(r#""{name} = 1""#)),
        &[&format!(
            "steps[0] -> script -> let[0]:\n\tname '{name}' exceeds 512 characters."
        )],
    );
}

// Characters, not bytes: 200 multi-byte characters are 600 bytes but well under
// the cap, so only the charset error should fire.
#[test]
fn test_let_name_multibyte_under_cap_reports_only_charset() {
    let name = "の".repeat(200);
    let v = yaml_val(&job_with_step_let(&format!(r#""{name} = 1""#)));
    let err = decode_job_template(v, Some(&["EXPR"]), &CallerLimits::default())
        .expect_err("Expected error");
    let msg = err.to_string();
    assert!(
        msg.contains("contains invalid characters."),
        "expected the charset error, got:\n{msg}"
    );
    assert!(
        !msg.contains("exceeds 512 characters."),
        "600 bytes is only 200 characters; the length error must not fire:\n{msg}"
    );
}

#[test]
fn failed_let_binding_does_not_cascade_into_membership() {
    // A failed binding is rebound as unresolved(ANY) so later bindings do not
    // cascade. Membership against it must therefore report only the first
    // error, not a second `Cannot use 'in' operator with list[int] and any`.
    let err = decode_job_template(
        yaml_val(&job_with_step_let(
            r#""Foo = 1 + 'x'", "Bar = Foo in [1, 2]""#,
        )),
        Some(&["EXPR"]),
        &CallerLimits::default(),
    )
    .expect_err("the first binding is invalid");
    let msg = err.to_string();
    assert!(
        msg.contains("Cannot use '+' operator with int and string"),
        "{msg}"
    );
    assert!(
        !msg.contains("Cannot use 'in' operator"),
        "membership against a failed binding cascaded:\n{msg}"
    );
}

/// Range-scope analogue of the job-creation path-format test
/// (`path_valued_let_binding_in_arg_passes_job_creation_checks`):
/// step-level `let` bindings are evaluated into the range symbol table
/// under `PathFormat::Posix`, and the range format strings that read
/// them must evaluate under the same format. Template validation used
/// to evaluate them under the host format, so a path-valued binding
/// referenced from a `parameterSpace` range drew "Path format
/// mismatch" on Windows (the formats coincide on Linux/macOS, where
/// this test is a no-op — the Windows CI lane is what exercises it).
#[test]
fn path_valued_step_let_in_range_passes_validation() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "let": ["out = path('/tmp/render/output.exr')"],
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "F", "type": "STRING", "range": ["{{ out.name }}", "b"]}
                ]
            },
            "script": {"actions": {"onRun": {"command": "echo"}}}
        }]
    }"#;
    decode_ok(s);
}

/// A step whose `let` bindings feed host requirements and a parameterSpace
/// range, for pinning every diagnostic those sites report and their order.
fn job_with_let_in_hr_and_range(let_bindings: &str, range_ref: &str) -> String {
    job_with_let_in_hr_and_range_ext(
        r#""EXPR", "FEATURE_BUNDLE_1""#,
        let_bindings,
        "b",
        range_ref,
    )
}

fn job_with_let_in_hr_and_range_ext(
    extensions: &str,
    let_bindings: &str,
    hr_ref: &str,
    range_ref: &str,
) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": [{extensions}],
        "parameterDefinitions": [{{"name": "N", "type": "INT", "default": 4}}],
        "steps": [{{
            "name": "S",
            "let": [{let_bindings}],
            "hostRequirements": {{
                "amounts": [{{"name": "amount.worker.vcpu", "min": "{{{{ {hr_ref} }}}}"}}]
            }},
            "parameterSpace": {{
                "taskParameterDefinitions": [
                    {{"name": "F", "type": "STRING", "range": ["{{{{ {range_ref} }}}}"]}}
                ]
            }},
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#
    )
}

fn full_err(s: &str) -> String {
    decode_job_template(
        yaml_val(s),
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err(&format!("Expected error for: {s}"))
    .to_string()
}

/// Bindings that evaluate are visible to host requirements and ranges.
#[test]
fn step_let_feeds_host_requirements_and_range() {
    decode_ok(&job_with_let_in_hr_and_range(
        r#""b = Param.N + 1", "t = 'x' * b""#,
        "t",
    ));
}

/// A binding that fails to evaluate is reported by the host-requirements
/// pass and the step body at `let[j]`, and by the range pass at the step,
/// where the failed name is unbound.
#[test]
fn step_let_evaluation_failure_diagnostics() {
    let msg = full_err(&job_with_let_in_hr_and_range(
        r#""b = Param.N + 1", "t = str(b)""#,
        "t",
    ));
    assert_eq!(
        msg,
        "Model validation error: 4 validation errors for JobTemplate\n\
         steps[0] -> let[1]:\n\
         \tInvalid expression in let binding 't': Unknown function: 'str'\n  \
         t = str(b)\n      \
         ^~~~~~\n\
         steps[0]:\n\
         \tlet binding 't': Unknown function: 'str'\n  \
         str(b)\n  \
         ^~~~~~\n\
         steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:\n\
         \tFailed to parse interpolation expression at [0, 7]. Undefined variable: 't'. Did you mean: b\n  \
         t\n  \
         ^\n\
         steps[0] -> let[1]:\n\
         \tInvalid expression in let binding 't': Unknown function: 'str'\n  \
         t = str(b)\n      \
         ^~~~~~"
    );
}

/// A structural error with no evaluation failure is reported by the
/// host-requirements pass and the step body; the range pass sees the
/// evaluated binding and reports nothing.
#[test]
fn step_let_structural_error_diagnostics() {
    let msg = full_err(&job_with_let_in_hr_and_range(
        r#""b = Param.N + 1", "Tt = 'x'""#,
        "Tt",
    ));
    assert_eq!(
        msg,
        "Model validation error: 2 validation errors for JobTemplate\n\
         steps[0] -> let[1]:\n\
         \tname 'Tt' must start with lowercase letter or underscore.\n\
         steps[0] -> let[1]:\n\
         \tname 'Tt' must start with lowercase letter or underscore."
    );
}

/// Let-binding errors are reported before the host-requirement errors of
/// the same step.
#[test]
fn step_let_errors_precede_host_requirement_errors() {
    let msg = full_err(&job_with_let_in_hr_and_range_ext(
        r#""EXPR", "FEATURE_BUNDLE_1""#,
        r#""b = Param.N + 1", "t = str(b)""#,
        "nope",
        "b",
    ));
    assert_eq!(
        msg,
        [
            "Model validation error: 4 validation errors for JobTemplate",
            "steps[0] -> let[1]:",
            "\tInvalid expression in let binding 't': Unknown function: 'str'",
            "  t = str(b)",
            "      ^~~~~~",
            "steps[0] -> hostRequirements -> amounts[0] -> min:",
            "\tFailed to parse interpolation expression at [0, 10]. Undefined variable: 'nope'. Did you mean one of: b, t",
            "  nope",
            "  ^~~~",
            "steps[0]:",
            "\tlet binding 't': Unknown function: 'str'",
            "  str(b)",
            "  ^~~~~~",
            "steps[0] -> let[1]:",
            "\tInvalid expression in let binding 't': Unknown function: 'str'",
            "  t = str(b)",
            "      ^~~~~~",
        ]
        .join("\n")
    );
}

/// When a binding fails, the range pass evaluates the bindings with
/// `Job.Name` and `Step.Name` in scope.
#[test]
fn step_let_failure_range_sees_job_and_step_names() {
    let msg = full_err(&job_with_let_in_hr_and_range_ext(
        r#""EXPR", "FEATURE_BUNDLE_1""#,
        r#""t = str(1)", "s = Step.Name + Job.Name""#,
        "1",
        "s",
    ));
    assert_eq!(
        msg,
        [
            "Model validation error: 3 validation errors for JobTemplate",
            "steps[0] -> let[0]:",
            "\tInvalid expression in let binding 't': Unknown function: 'str'",
            "  t = str(1)",
            "      ^~~~~~",
            "steps[0]:",
            "\tlet binding 't': Unknown function: 'str'",
            "  str(1)",
            "  ^~~~~~",
            "steps[0] -> let[0]:",
            "\tInvalid expression in let binding 't': Unknown function: 'str'",
            "  t = str(1)",
            "      ^~~~~~",
        ]
        .join("\n")
    );
}

/// When a binding fails, the range pass skips bindings with no `=`, an
/// empty name, or an empty expression.
#[test]
fn step_let_failure_range_skips_malformed_bindings() {
    let msg = full_err(&job_with_let_in_hr_and_range_ext(
        r#""EXPR", "FEATURE_BUNDLE_1""#,
        r#""t = str(1)", "c =", "= 2", "d 3""#,
        "1",
        "'x'",
    ));
    let let_errors = [
        "steps[0] -> let[0]:",
        "\tInvalid expression in let binding 't': Unknown function: 'str'",
        "  t = str(1)",
        "      ^~~~~~",
        "steps[0] -> let[1]:",
        "\tbinding 'c' has no expression after '='.",
        "steps[0] -> let[2]:",
        "\thas empty name.",
        "steps[0] -> let[3]:",
        "\tmissing '=' in 'd 3'.",
    ];
    let mut expected = vec!["Model validation error: 9 validation errors for JobTemplate"];
    expected.extend(let_errors);
    expected.extend([
        "steps[0]:",
        "\tlet binding 't': Unknown function: 'str'",
        "  str(1)",
        "  ^~~~~~",
    ]);
    expected.extend(let_errors);
    assert_eq!(msg, expected.join("\n"));
}

/// Without EXPR, step-level bindings are rejected and are not visible to
/// host requirements or ranges.
#[test]
fn step_let_without_expr_is_not_in_scope() {
    let msg = full_err(&job_with_let_in_hr_and_range_ext(
        r#""FEATURE_BUNDLE_1""#,
        r#""b = 1""#,
        "b",
        "b",
    ));
    assert_eq!(
        msg,
        [
            "Model validation error: 3 validation errors for JobTemplate",
            "steps[0] -> hostRequirements -> amounts[0] -> min:",
            "\tFailed to parse interpolation expression at [0, 7]. Undefined variable: 'b'.",
            "  b",
            "  ^",
            "steps[0] -> parameterSpace -> taskParameterDefinitions[0] -> range[0]:",
            "\tFailed to parse interpolation expression at [0, 7]. Undefined variable: 'b'.",
            "  b",
            "  ^",
            "steps[0] -> let:",
            "\t'let' requires the EXPR extension.",
        ]
        .join("\n")
    );
}

/// When a binding fails to parse, the range pass reports the parse error at
/// the step.
#[test]
fn step_let_parse_failure_diagnostics() {
    let msg = full_err(&job_with_let_in_hr_and_range_ext(
        r#""EXPR", "FEATURE_BUNDLE_1""#,
        r#""x = (""#,
        "1",
        "'a'",
    ));
    let let_error = [
        "steps[0] -> let[0]:",
        "\tInvalid expression in let binding 'x': Syntax error: unexpected EOF while parsing",
        "  x = (",
        "      ^",
    ];
    let mut expected = vec!["Model validation error: 3 validation errors for JobTemplate"];
    expected.extend(let_error);
    expected.extend([
        "steps[0]:",
        "\tlet binding 'x': Syntax error: unexpected EOF while parsing",
        "  (",
        "  ^",
    ]);
    expected.extend(let_error);
    assert_eq!(msg, expected.join("\n"));
}

/// Each step's host requirements see that step's own bindings, and errors
/// come in this order: every step's host requirements, then the job
/// environments, then each step's remaining checks.
#[test]
fn step_let_scopes_and_error_order_across_steps() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "jobEnvironments": [{"name": "J", "variables": {"V": "{{ nope_env }}"}}],
        "steps": [
            {
                "name": "A",
                "let": ["v = 2"],
                "hostRequirements": {
                    "amounts": [{"name": "amount.worker.vcpu", "min": "{{ v }}", "max": "{{ nope_a }}"}]
                },
                "script": {"actions": {"onRun": {"command": "echo", "args": ["{{ v }}", "{{ nope_a2 }}"]}}}
            },
            {
                "name": "B",
                "let": ["w = 'x'"],
                "hostRequirements": {
                    "amounts": [{"name": "amount.worker.vcpu", "min": "{{ w }}"}]
                },
                "script": {"actions": {"onRun": {"command": "echo", "args": ["{{ w }}", "{{ nope_b }}"]}}}
            },
            {
                "name": "C",
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }
        ]
    }"#;
    assert_eq!(
        full_err(s),
        [
            "Model validation error: 5 validation errors for JobTemplate",
            "steps[0] -> hostRequirements -> amounts[0] -> max:",
            "\tFailed to parse interpolation expression at [0, 12]. Undefined variable: 'nope_a'.",
            "  nope_a",
            "  ^~~~~~",
            "steps[1] -> hostRequirements -> amounts[0] -> min:",
            "\tFailed to parse interpolation expression at [0, 7]. Cannot coerce string to float?",
            "  w",
            "  ^",
            "jobEnvironments[0] -> variables -> V:",
            "\tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'nope_env'.",
            "  nope_env",
            "  ^~~~~~~~",
            "steps[0] -> script -> actions -> onRun -> args[1]:",
            "\tFailed to parse interpolation expression at [0, 13]. Undefined variable: 'nope_a2'.",
            "  nope_a2",
            "  ^~~~~~~",
            "steps[1] -> script -> actions -> onRun -> args[1]:",
            "\tFailed to parse interpolation expression at [0, 12]. Undefined variable: 'nope_b'.",
            "  nope_b",
            "  ^~~~~~",
        ]
        .join("\n")
    );
}

/// After a binding fails, the step body still sees every binding: those
/// that evaluated with their values, the failed one as unresolved.
#[test]
fn step_let_failure_body_sees_evaluated_bindings() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "steps": [{
            "name": "S",
            "let": ["b = 1", "t = str(b)"],
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "F", "type": "INT", "range": "1-{{ b }}"}]
            },
            "script": {"actions": {"onRun": {"command": "echo", "args": ["{{ b }}", "{{ t }}"]}}}
        }]
    }"#;
    assert_eq!(
        full_err(s),
        [
            "Model validation error: 2 validation errors for JobTemplate",
            "steps[0]:",
            "\tlet binding 't': Unknown function: 'str'",
            "  str(b)",
            "  ^~~~~~",
            "steps[0] -> let[1]:",
            "\tInvalid expression in let binding 't': Unknown function: 'str'",
            "  t = str(b)",
            "      ^~~~~~",
        ]
        .join("\n")
    );
}

/// After a binding fails, the step body's template-scope fields still see
/// `Step.Name` and the bindings that read it.
#[test]
fn step_let_failure_timeout_sees_step_name() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "steps": [{
            "name": "S",
            "let": ["s = Step.Name", "b = int('nope')"],
            "parameterSpace": {
                "taskParameterDefinitions": [{"name": "F", "type": "INT", "range": "1-3"}]
            },
            "script": {"actions": {"onRun": {
                "command": "echo",
                "timeout": "{{ len(Step.Name) + len(s) }}"
            }}}
        }]
    }"#;
    assert_eq!(
        full_err(s),
        [
            "Model validation error: 2 validation errors for JobTemplate",
            "steps[0]:",
            "\tlet binding 'b': Cannot convert 'nope' to int",
            "  int('nope')",
            "  ^~~~~~~~~~~",
            "steps[0] -> let[1]:",
            "\tInvalid expression in let binding 'b': Cannot convert 'nope' to int",
            "  b = int('nope')",
            "      ^~~~~~~~~~~",
        ]
        .join("\n")
    );
}

/// A step's body sees its own bindings, not those of a later step whose
/// host requirements were checked with its bindings.
#[test]
fn step_let_body_sees_only_its_own_bindings() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "steps": [
            {
                "name": "A",
                "let": ["c = 1"],
                "script": {"actions": {"onRun": {"command": "echo", "args": ["{{ b }}"]}}}
            },
            {
                "name": "B",
                "let": ["b = 1"],
                "hostRequirements": {
                    "amounts": [{"name": "amount.worker.vcpu", "min": "{{ b }}"}]
                },
                "script": {"actions": {"onRun": {"command": "echo"}}}
            }
        ]
    }"#;
    assert_eq!(
        full_err(s),
        [
            "Model validation error: 1 validation error for JobTemplate",
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "\tFailed to parse interpolation expression at [0, 7]. Undefined variable: 'b'. Did you mean: c",
            "  b",
            "  ^",
        ]
        .join("\n")
    );
}

/// Bindings shared from host requirements leave `Job.Name` and `Step.Name`
/// in scope for the step's ranges and body.
#[test]
fn step_let_shared_scope_keeps_job_and_step_names() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR", "FEATURE_BUNDLE_1"],
        "steps": [{
            "name": "S",
            "let": ["a = 1"],
            "hostRequirements": {
                "amounts": [{"name": "amount.worker.vcpu", "min": 1}]
            },
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "F", "type": "STRING", "range": ["{{ Job.Name }}-{{ Step.Name }}"]}
                ]
            },
            "script": {"actions": {"onRun": {
                "command": "echo",
                "args": ["{{ a }}"],
                "timeout": "{{ len(Step.Name) + 10 }}"
            }}}
        }]
    }"#,
    );
}

// ── Expression type depth (MAX_TYPE_DEPTH) ──

/// `binding` nesting `inner` in `n` list comprehensions with an unknown
/// filter, each typed `list[T | nulltype]`, two type levels over `T`.
fn nested_comprehensions(inner: &str, n: usize) -> String {
    (0..n).fold(inner.to_string(), |c, i| {
        format!("[{c} for v{i} in [1] if Param.C]")
    })
}

fn job_with_params_and_script_let(let_bindings: &[String]) -> String {
    let bindings: Vec<String> = let_bindings
        .iter()
        .map(|b| serde_json::to_string(b).unwrap())
        .collect();
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {{"name": "N", "type": "INT", "default": 1}},
            {{"name": "C", "type": "BOOL", "default": false}}
        ],
        "steps": [{{
            "name": "S",
            "script": {{
                "let": [{}],
                "actions": {{"onRun": {{"command": "foo"}}}}
            }}
        }}]
    }}"#,
        bindings.join(", ")
    )
}

/// The deepest comprehension nesting one expression allows (62 levels
/// inside `len`, within the 64-level expression depth limit, typed 125
/// deep) validates: the type depth limit is above it.
#[test]
fn deepest_single_expression_comprehension_validates() {
    let decode = |n: usize| {
        decode_job_template(
            yaml_val(&job_with_params_and_script_let(&[format!(
                "x = len({})",
                nested_comprehensions("Param.N", n)
            )])),
            Some(&["EXPR"]),
            &CallerLimits::default(),
        )
    };
    // One more level is over the expression depth limit, not the type
    // depth limit.
    let err = decode(63).expect_err("63 levels inside len() are 65 deep");
    assert!(
        err.to_string()
            .contains("Expression nesting depth (65) exceeded limit (64)"),
        "{err}"
    );
    decode(62).expect("the deepest nesting validates");
}

/// A chain of bindings that each nest the one before builds a type past
/// the limit; the binding that passes it fails validation.
#[test]
fn let_chain_past_the_type_depth_limit_fails_at_that_binding() {
    let mut bindings = vec!["a0 = Param.N".to_string()];
    for i in 1..=3 {
        bindings.push(format!(
            "a{i} = {}",
            nested_comprehensions(&format!("a{}", i - 1), 30)
        ));
    }
    let start = std::time::Instant::now();
    check_err(
        &job_with_params_and_script_let(&bindings),
        &[
            "steps[0] -> script -> let[3]:",
            "Invalid expression in let binding 'a3': Type nesting depth exceeded limit (160)",
        ],
    );
    // A chain of this length validated in about a second without the
    // limit, which grows to minutes over a longer chain (the next test).
    assert!(
        start.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

/// A format string field (here a command's argument) whose expression
/// types past the limit fails validation at that field, with the
/// expression and a caret, as a `let` binding does.
#[test]
fn format_string_past_the_type_depth_limit_fails_at_that_field() {
    let template = |arg: &str| {
        format!(
            r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "Test",
        "extensions": ["EXPR"],
        "parameterDefinitions": [
            {{"name": "N", "type": "INT", "default": 1}},
            {{"name": "C", "type": "BOOL", "default": false}}
        ],
        "steps": [{{
            "name": "S",
            "script": {{
                "let": [{}, {}],
                "actions": {{"onRun": {{"command": "foo", "args": ["{arg}"]}}}}
            }}
        }}]
    }}"#,
            serde_json::to_string(&format!("b1 = {}", nested_comprehensions("Param.N", 60)))
                .unwrap(),
            serde_json::to_string(&format!("b2 = {}", nested_comprehensions("b1", 19))).unwrap(),
        )
    };
    // `b2` is 159 deep, so `[b2]` is at the limit and `[[b2]]` past it.
    decode_ok(&template("{{ len([b2]) }}"));
    let err = decode_job_template(
        yaml_val(&template("{{ len([[b2]]) }}")),
        Some(&["EXPR", "FEATURE_BUNDLE_1"]),
        &CallerLimits::default(),
    )
    .expect_err("[[b2]] is past the limit")
    .to_string();
    // The whole error, so a caret too long or on another node fails.
    assert_eq!(
        err,
        "Model validation error: 1 validation error for JobTemplate\n\
         steps[0] -> script -> actions -> onRun -> args[0]:\n\
         \tFailed to parse interpolation expression at [0, 17]. \
         Type nesting depth exceeded limit (160)\n  len([[b2]])\n      ^~~~~~"
    );
}

/// A chain as long as `let` allows (50 bindings), each nesting the one
/// before in 30 comprehensions, also fails at the first binding past the
/// limit. Each later binding is typed too (a failed one is rebound as
/// `unresolved[any]`), which takes about 10 s in a debug build; without the
/// limit a release build took over 5 minutes.
#[test]
fn longest_let_chain_past_the_type_depth_limit_fails_at_that_binding() {
    let mut bindings = vec!["a0 = Param.N".to_string()];
    for i in 1..50 {
        bindings.push(format!(
            "a{i} = {}",
            nested_comprehensions(&format!("a{}", i - 1), 30)
        ));
    }
    let start = std::time::Instant::now();
    check_err(
        &job_with_params_and_script_let(&bindings),
        &[
            "steps[0] -> script -> let[3]:",
            "Invalid expression in let binding 'a3': Type nesting depth exceeded limit (160)",
        ],
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(60),
        "{:?}",
        start.elapsed()
    );
}
