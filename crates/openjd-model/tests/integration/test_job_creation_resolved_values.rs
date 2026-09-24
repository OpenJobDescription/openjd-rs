// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Resolved-value checks at job creation (`create_job`) — the
//! carried-forward session/task-scope format strings: action
//! `command`/`args`, environment `variables` values, and embedded-file
//! `data`. See the Resolved-Value Checks on Carried-Forward Fields
//! section of `specs/model/job-creation.md`.
//!
//! Every violation here depends only on job parameter values, so it is
//! not statically knowable at template validation (which sees an
//! unresolved `Param.*` and a lower bound of 0) but is fully decidable
//! at job creation, when the parameters are bound — before any worker
//! runs a task.
//!
//! Failure tests assert the full field path + message per the repo's
//! error-message test standard. Passing controls keep partially
//! unresolved strings (contributing 0 to the bound) accepted.

use openjd_expr::path_mapping::PathFormat;
use openjd_model::{create_job, decode_job_template, job, preprocess_job_parameters, CallerLimits};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

/// Decode with the *default* (uncapped) caller limits — so template
/// validation passes — then run `create_job` with `limits`, mirroring a service
/// that enforces stricter caller limits at submission than at check.
fn create_with_limits(
    template_json: &str,
    params: &[(&str, &str)],
    limits: CallerLimits,
) -> Result<job::Job, String> {
    let root = tempfile::TempDir::new().unwrap();
    let dir = root.path().to_str().unwrap();
    let v = yaml_val(template_json);
    let jt = decode_job_template(
        v,
        Some(&["EXPR", "FEATURE_BUNDLE_1", "WRAP_ACTIONS"]),
        &CallerLimits::default(),
    )
    .expect("template must pass validation under default limits");
    let input: std::collections::HashMap<String, openjd_expr::ExprValue> = params
        .iter()
        .map(|(k, v)| (k.to_string(), openjd_expr::ExprValue::String(v.to_string())))
        .collect();
    let processed = preprocess_job_parameters(
        &jt,
        &input,
        &[],
        &openjd_model::PathParameterOptions {
            job_template_dir: dir,
            current_working_dir: dir,
            allow_template_dir_walk_up: true,
            path_format: PathFormat::host(),
            allow_uri_path_values: true,
        },
    )
    .map_err(|e| e.to_string())?;
    let mut ctx = jt.default_validation_context();
    ctx.caller_limits = limits;
    create_job(&jt, &processed, &ctx).map_err(|e| e.to_string())
}

fn create_default(template_json: &str, params: &[(&str, &str)]) -> Result<job::Job, String> {
    create_with_limits(template_json, params, CallerLimits::default())
}

fn assert_err_contains(result: Result<job::Job, String>, expected: &[&str]) {
    let msg = result.expect_err("expected create_job to fail");
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

// ══════════════════════════════════════════════════════════════
// §4.4.2 environment variable values — ≤ 2048 resolved characters
// (spec-mandated, always on)
// ══════════════════════════════════════════════════════════════

fn job_env_var_template(value: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{"name": "Env", "variables": {{"FOO": "{value}"}}}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#
    )
}

#[test]
fn job_env_variable_over_2048_from_param_fails_at_create_job() {
    // Template validation sees an unresolved Param.X (lower bound 0)
    // and passes; the bound value makes the violation decidable at job
    // creation.
    assert_err_contains(
        create_default(
            &job_env_var_template("{{Param.X}}"),
            &[("X", &"A".repeat(3000))],
        ),
        &[
            "jobEnvironments[0] -> variables -> FOO:",
            "resolves to at least 3000 characters, exceeding the maximum of 2048.",
        ],
    );
}

#[test]
fn job_env_variable_under_limit_passes() {
    create_default(&job_env_var_template("{{Param.X}}"), &[("X", "short")])
        .expect("under-limit value must pass");
}

#[test]
fn job_env_variable_with_unresolved_session_part_passes() {
    // Session.WorkingDirectory is only known on the worker: it
    // contributes 0 to the bound, and the concrete part is under 2048.
    create_default(
        &job_env_var_template("{{Session.WorkingDirectory}}/{{Param.X}}"),
        &[("X", "short")],
    )
    .expect("partially unresolved under-limit value must pass");
}

#[test]
fn job_env_variable_bound_over_limit_fails_despite_unresolved_part() {
    // The unresolved segment cannot shrink the resolved value below the
    // concrete segment's contribution.
    assert_err_contains(
        create_default(
            &job_env_var_template("{{Session.WorkingDirectory}}/{{Param.X}}"),
            &[("X", &"A".repeat(3000))],
        ),
        &[
            "jobEnvironments[0] -> variables -> FOO:",
            "resolves to at least 3001 characters, exceeding the maximum of 2048.",
        ],
    );
}

#[test]
fn step_env_variable_over_2048_from_param_fails_at_create_job() {
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "stepEnvironments": [{{"name": "Env", "variables": {{"FOO": "{}"}}}}],
            "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}
        }}]
    }}"#,
        "{{Param.X}}"
    );
    assert_err_contains(
        create_default(&template, &[("X", &"A".repeat(3000))]),
        &[
            "steps[0] -> stepEnvironments[0] -> variables -> FOO:",
            "resolves to at least 3000 characters, exceeding the maximum of 2048.",
        ],
    );
}

#[test]
fn env_let_binding_value_flows_into_variable_check() {
    // The environment's `let` binding is evaluated into the check
    // symbol table, so a variable interpolating it is fully static once
    // parameters are bound.
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{
            "name": "Env",
            "variables": {{"FOO": "{}"}},
            "script": {{
                "let": ["doubled = Param.X + Param.X"],
                "actions": {{"onEnter": {{"command": "echo"}}}}
            }}
        }}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#,
        "{{doubled}}"
    );
    assert_err_contains(
        create_default(&template, &[("X", &"A".repeat(1500))]),
        &[
            "jobEnvironments[0] -> variables -> FOO:",
            "resolves to at least 3000 characters, exceeding the maximum of 2048.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// §5.1/§5.2 command and args — opt-in CallerLimits::max_resolved_arg_len
// ══════════════════════════════════════════════════════════════

fn arg_template(arg: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [
            {{"name": "X", "type": "STRING"}},
            {{"name": "N", "type": "INT"}}
        ],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo", "args": ["{arg}"]}}}}}}}}]
    }}"#
    )
}

fn arg_cap(n: usize) -> CallerLimits {
    CallerLimits {
        max_resolved_arg_len: Some(n),
        ..Default::default()
    }
}

#[test]
fn arg_over_cap_from_param_fails_at_create_job() {
    assert_err_contains(
        create_with_limits(
            &arg_template("{{Param.X}}"),
            &[("X", &"A".repeat(200)), ("N", "1")],
            arg_cap(100),
        ),
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "resolves to at least 200 characters, exceeding the maximum of 100.",
        ],
    );
}

#[test]
fn arg_expression_blowup_from_int_param_fails_at_create_job() {
    // `'A' * Param.N` is unbounded at template validation and
    // decidable the moment N is bound.
    assert_err_contains(
        create_with_limits(
            &arg_template("{{ 'A' * Param.N }}"),
            &[("X", "x"), ("N", "100000")],
            arg_cap(1024),
        ),
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "resolves to at least 100000 characters, exceeding the maximum of 1024.",
        ],
    );
}

#[test]
fn arg_under_cap_passes() {
    create_with_limits(
        &arg_template("{{Param.X}}"),
        &[("X", "short"), ("N", "1")],
        arg_cap(100),
    )
    .expect("under-cap arg must pass");
}

#[test]
fn arg_with_unresolved_task_part_contributes_zero() {
    // Task.Param values are unknown until a session runs a task; only
    // the concrete parts count toward the bound.
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "parameterSpace": {{"taskParameterDefinitions": [{{"name": "Frame", "type": "INT", "range": "1-10"}}]}},
            "script": {{"actions": {{"onRun": {{"command": "echo", "args": ["--frame={}-{}"]}}}}}}
        }}]
    }}"#,
        "{{Task.Param.Frame}}", "{{Param.X}}"
    );
    create_with_limits(&template, &[("X", "short")], arg_cap(100))
        .expect("bound counts only concrete segments");
}

#[test]
fn arg_without_cap_passes_whatever_the_length() {
    // Group B: §5.2 sets no maximum — the default posture stays uncapped.
    create_default(
        &arg_template("{{Param.X}}"),
        &[("X", &"A".repeat(5000)), ("N", "1")],
    )
    .expect("no cap, no limit beyond the spec");
}

#[test]
fn command_over_cap_from_param_fails_at_create_job() {
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "{}"}}}}}}}}]
    }}"#,
        "{{Param.X}}"
    );
    assert_err_contains(
        create_with_limits(&template, &[("X", &"A".repeat(200))], arg_cap(100)),
        &[
            "steps[0] -> script -> actions -> onRun -> command:",
            "resolves to at least 200 characters, exceeding the maximum of 100.",
        ],
    );
}

#[test]
fn env_action_arg_over_cap_fails_at_create_job() {
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{
            "name": "Env",
            "script": {{"actions": {{"onEnter": {{"command": "echo", "args": ["{}"]}}}}}}
        }}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#,
        "{{Param.X}}"
    );
    assert_err_contains(
        create_with_limits(&template, &[("X", &"A".repeat(200))], arg_cap(100)),
        &[
            "jobEnvironments[0] -> script -> actions -> onEnter -> args[0]:",
            "resolves to at least 200 characters, exceeding the maximum of 100.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// §6.1.2 embedded file data — opt-in CallerLimits::max_resolved_data_len
// ══════════════════════════════════════════════════════════════

fn data_cap(n: usize) -> CallerLimits {
    CallerLimits {
        max_resolved_data_len: Some(n),
        ..Default::default()
    }
}

fn step_data_template() -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{
            "name": "S",
            "script": {{
                "embeddedFiles": [{{"name": "F", "type": "TEXT", "data": "{}"}}],
                "actions": {{"onRun": {{"command": "echo"}}}}
            }}
        }}]
    }}"#,
        "{{Param.X}}"
    )
}

#[test]
fn embedded_file_data_over_cap_from_param_fails_at_create_job() {
    assert_err_contains(
        create_with_limits(
            &step_data_template(),
            &[("X", &"A".repeat(200))],
            data_cap(100),
        ),
        &[
            "steps[0] -> script -> embeddedFiles[0] -> data:",
            "resolves to at least 200 characters, exceeding the maximum of 100.",
        ],
    );
}

#[test]
fn embedded_file_data_under_cap_passes() {
    create_with_limits(&step_data_template(), &[("X", "short")], data_cap(100))
        .expect("under-cap data must pass");
}

#[test]
fn env_embedded_file_data_over_cap_fails_at_create_job() {
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{
            "name": "Env",
            "script": {{
                "embeddedFiles": [{{"name": "F", "type": "TEXT", "data": "{}"}}],
                "actions": {{"onEnter": {{"command": "echo"}}}}
            }}
        }}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#,
        "{{Param.X}}"
    );
    assert_err_contains(
        create_with_limits(&template, &[("X", &"A".repeat(200))], data_cap(100)),
        &[
            "jobEnvironments[0] -> script -> embeddedFiles[0] -> data:",
            "resolves to at least 200 characters, exceeding the maximum of 100.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Environment action coverage: onExit and the RFC 0008 wrap hooks
// ══════════════════════════════════════════════════════════════

#[test]
fn env_on_exit_arg_over_cap_fails_at_create_job() {
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{
            "name": "Env",
            "script": {{"actions": {{
                "onEnter": {{"command": "echo"}},
                "onExit": {{"command": "echo", "args": ["{}"]}}
            }}}}
        }}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#,
        "{{Param.X}}"
    );
    assert_err_contains(
        create_with_limits(&template, &[("X", &"A".repeat(200))], arg_cap(100)),
        &[
            "jobEnvironments[0] -> script -> actions -> onExit -> args[0]:",
            "resolves to at least 200 characters, exceeding the maximum of 100.",
        ],
    );
}

#[test]
fn wrap_hook_arg_over_cap_fails_at_create_job() {
    // The wrap hooks see their WrappedAction.* scope (unresolved,
    // contributing 0) alongside the concrete parameters.
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["WRAP_ACTIONS", "EXPR"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "jobEnvironments": [{{
            "name": "Wrapper",
            "script": {{"actions": {{
                "onWrapEnvEnter": {{"command": "echo"}},
                "onWrapTaskRun": {{"command": "{}", "args": ["{}"]}},
                "onWrapEnvExit": {{"command": "echo"}}
            }}}}
        }}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#,
        "{{WrappedAction.Command}}", "--tag={{Param.X}}"
    );
    assert_err_contains(
        create_with_limits(&template, &[("X", &"A".repeat(200))], arg_cap(100)),
        &[
            "jobEnvironments[0] -> script -> actions -> onWrapTaskRun -> args[0]:",
            "resolves to at least 206 characters, exceeding the maximum of 100.",
        ],
    );
}

// ══════════════════════════════════════════════════════════════
// Evaluation budgets — CallerLimits::max_eval_memory_bytes applies to
// job creation's evaluations, as it does to template validation and
// the session runtime
// ══════════════════════════════════════════════════════════════

#[test]
fn lowered_memory_budget_fails_carried_forward_expression_at_create_job() {
    // `'A' * Param.N` with a large bound N exceeds a lowered memory
    // budget during the job-creation evaluation — the spec's own lever
    // against expression blowups, applied at this stage too.
    assert_err_contains(
        create_with_limits(
            &arg_template("{{ 'A' * Param.N }}"),
            &[("X", "x"), ("N", "10000000")],
            CallerLimits {
                max_eval_memory_bytes: Some(1024 * 1024),
                ..Default::default()
            },
        ),
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "Expression memory usage (10000136 bytes) exceeded limit (1048576 bytes)",
        ],
    );
}

#[test]
fn lowered_memory_budget_fails_job_name_resolution() {
    let template = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "{}",
        "parameterDefinitions": [{{"name": "N", "type": "INT"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "echo"}}}}}}}}]
    }}"#,
        "{{ 'A' * Param.N }}"
    );
    let err = create_with_limits(
        &template,
        &[("N", "10000000")],
        CallerLimits {
            max_eval_memory_bytes: Some(1024 * 1024),
            ..Default::default()
        },
    )
    .expect_err("expected job name resolution to exceed the memory budget");
    assert!(err.contains("Failed to resolve job name"), "Got:\n{err}");
}

// ══════════════════════════════════════════════════════════════
// Evaluation-error reporting — `create_job` requires a context
// enabling the template's declared extensions, so every evaluation
// error at this stage is a template defect or a deterministic
// value-dependent failure, and all of them are reported.
// ══════════════════════════════════════════════════════════════

#[test]
fn value_dependent_evaluation_error_fails_at_create_job() {
    // Pass 8 sees Param.X unresolved and cannot evaluate; with X bound
    // to "0" the division fails deterministically — the same failure
    // every session would hit at resolution. Reported here instead.
    assert_err_contains(
        create_default(
            &arg_template("{{ 1 / int(Param.X) }}"),
            &[("X", "0"), ("N", "1")],
        ),
        &[
            "steps[0] -> script -> actions -> onRun -> args[0]:",
            "Division by zero\n",
            "  1 / int(Param.X)\n",
            "  ~~^~~~~~~~~~~~~~",
        ],
    );
    // Control: a non-zero value evaluates cleanly.
    create_default(
        &arg_template("{{ 1 / int(Param.X) }}"),
        &[("X", "2"), ("N", "1")],
    )
    .expect("non-zero divisor must pass");
}

#[test]
fn budget_error_inside_unresolved_conditional_is_reported() {
    // With an unresolved test (Session.*), the evaluator runs both
    // branches; a budget exceedance in either propagates even when the
    // other branch succeeds, because the memory was spent in this
    // evaluation no matter which branch run time takes (see IfExp in
    // specs/expr/evaluator.md). This pins that a caller who lowers
    // max_eval_memory_bytes is protected inside conditionals — the
    // idiomatic construction, since Session.* tests are unresolved at
    // job creation.
    let err = create_with_limits(
        &arg_template("{{ 'A' * Param.N if Session.HasPathMappingRules else 'B' }}"),
        &[("X", "x"), ("N", "10000000")],
        CallerLimits {
            max_eval_memory_bytes: Some(1024 * 1024),
            ..Default::default()
        },
    )
    .expect_err("expected the branch's budget exceedance to be reported");
    assert!(
        err.contains("steps[0] -> script -> actions -> onRun -> args[0]:"),
        "Got:\n{err}"
    );
    assert!(
        err.contains("Expression memory usage (10000136 bytes) exceeded limit (1048576 bytes)"),
        "Got:\n{err}"
    );
}

/// The job-creation checks evaluate under the POSIX path format —
/// matching every other evaluation `create_job` performs, including
/// the `let` bindings seeded into the check symbol tables. A
/// host-format evaluation would reject the Posix-format path values in
/// the symtab with a "Path format mismatch" error on Windows (the
/// formats coincide elsewhere), so this pins the format agreement
/// between the checks and the symtabs they read.
#[test]
fn path_valued_let_binding_in_arg_passes_job_creation_checks() {
    let template = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "Test",
        "parameterDefinitions": [
            {"name": "X", "type": "STRING"},
            {"name": "N", "type": "INT"}
        ],
        "steps": [{"name": "S", "script": {
            "let": ["out = path('/tmp/render/output.exr')"],
            "actions": {"onRun": {"command": "echo", "args": ["{{ out.name }}"]}}
        }}]
    }"#;
    create_default(template, &[("X", "x"), ("N", "1")])
        .expect("Posix path values in the check symtab must evaluate cleanly");
}

// ══════════════════════════════════════════════════════════════
// Partial resolution must not reject templates that validate and run
// ══════════════════════════════════════════════════════════════

/// Job creation evaluates under a symbol-table state that exists at no
/// other stage: `Param.*` concrete, `Task.*`/`Session.*` unresolved. A
/// comprehension over a now-concrete iterable whose filter references a
/// task parameter is undecidable per element here — but validates at
/// template validation (iterable unresolved) and resolves cleanly in
/// every session (everything bound). It must pass job creation as an
/// unresolved value, not fail it. Regression test: the concrete-
/// iterable path in `eval_listcomp` used to hard-error on an
/// unresolved filter condition, rejecting this template at submission.
#[test]
fn listcomp_with_task_param_filter_over_bound_param_passes_create_job() {
    let template = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["EXPR"],
        "name": "LcFilter",
        "parameterDefinitions": [
            {"name": "Files", "type": "STRING"},
            {"name": "X", "type": "STRING"},
            {"name": "N", "type": "INT"}
        ],
        "steps": [{
            "name": "Render",
            "parameterSpace": {
                "taskParameterDefinitions": [
                    {"name": "Skip", "type": "STRING", "range": ["b"]}
                ]
            },
            "script": {"actions": {"onRun": {
                "command": "echo",
                "args": ["{{ [f for f in Param.Files.split(',') if f != Task.Param.Skip] }}"]
            }}}
        }]
    }"#;
    create_default(template, &[("Files", "a,b,c"), ("X", "x"), ("N", "1")])
        .expect("an unresolved comprehension filter must not fail job creation");
}
