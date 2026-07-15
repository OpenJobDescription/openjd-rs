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
