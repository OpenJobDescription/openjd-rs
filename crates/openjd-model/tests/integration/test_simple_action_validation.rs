// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! SimpleAction (§8, FEATURE_BUNDLE_1) validation: the desugared script is
//! checked by exactly the same code as an authored `script`, at both
//! template validation and job creation — and every diagnostic is reported
//! at the field the author wrote (`steps[i] -> bash -> script`,
//! `-> args[k]`, `-> timeout`, ...), never at a node of the desugared form
//! (`steps[i] -> script -> embeddedFiles[0] -> data`).
//!
//! Failure tests assert the full field path + message per the repo's
//! error-message test standard.

use openjd_expr::path_mapping::PathFormat;
use openjd_model::{create_job, decode_job_template, job, preprocess_job_parameters, CallerLimits};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

const EXTS: &[&str] = &["EXPR", "FEATURE_BUNDLE_1"];

fn decode_with(
    s: &str,
    limits: &CallerLimits,
) -> Result<openjd_model::template::JobTemplate, String> {
    decode_job_template(yaml_val(s), Some(EXTS), limits).map_err(|e| e.to_string())
}

fn decode_ok(s: &str) {
    decode_with(s, &CallerLimits::default())
        .unwrap_or_else(|e| panic!("Expected success for:\n{s}\nGot:\n{e}"));
}

fn assert_contains(msg: &str, expected: &[&str]) {
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn check_err(s: &str, expected: &[&str]) {
    let msg = decode_with(s, &CallerLimits::default())
        .err()
        .unwrap_or_else(|| panic!("Expected error for:\n{s}"));
    assert_contains(&msg, expected);
}

fn check_err_with(s: &str, limits: &CallerLimits, expected: &[&str]) {
    let msg = decode_with(s, limits)
        .err()
        .unwrap_or_else(|| panic!("Expected error for:\n{s}"));
    assert_contains(&msg, expected);
}

/// Decode under default limits (so template validation passes), then run
/// `create_job` under `limits` — the "stricter at submission" pattern.
fn create_with_limits(
    template: &str,
    params: &[(&str, &str)],
    limits: CallerLimits,
) -> Result<job::Job, String> {
    let root = tempfile::TempDir::new().unwrap();
    let dir = root.path().to_str().unwrap();
    let jt = decode_with(template, &CallerLimits::default())
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

fn data_cap(n: usize) -> CallerLimits {
    CallerLimits {
        max_resolved_data_len: Some(n),
        ..CallerLimits::default()
    }
}

fn arg_cap(n: usize) -> CallerLimits {
    CallerLimits {
        max_resolved_arg_len: Some(n),
        ..CallerLimits::default()
    }
}

// ══════════════════════════════════════════════════════════════
// Template validation — format-string references (pass 8)
// ══════════════════════════════════════════════════════════════

#[test]
fn undefined_variable_in_script_reported_at_sugar_script() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": "echo {{Param.Nope}}"}}]
    }"#,
        &[
            "steps[0] -> bash -> script:\n\tFailed to parse interpolation expression at [5, 19]. Undefined variable: 'Param.Nope'.",
        ],
    );
}

#[test]
fn undefined_variable_in_args_reported_at_authored_index() {
    // The desugared `args` is [<file ref>, "ok", "{{Param.Nope}}"]; the
    // author wrote args[1], so that is what the path must say.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": "echo", "args": ["ok", "{{Param.Nope}}"]}}]
    }"#,
        &[
            "steps[0] -> bash -> args[1]:\n\tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'Param.Nope'.",
        ],
    );
}

#[test]
fn args_index_accounts_for_interpreter_prefix() {
    // cmd desugars to ["/C", <file ref>, ...authored]; powershell to
    // ["-File", <file ref>, ...authored]. Authored args[0] must stay args[0].
    for kind in ["cmd", "powershell"] {
        let s = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "extensions": ["FEATURE_BUNDLE_1"],
            "name": "Test",
            "steps": [{{"name": "S", "{kind}": {{"script": "echo", "args": ["{{{{Param.Nope}}}}"]}}}}]
        }}"#
        );
        check_err(
            &s,
            &[&format!(
                "steps[0] -> {kind} -> args[0]:\n\tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'Param.Nope'."
            )],
        );
        assert!(
            !decode_with(&s, &CallerLimits::default())
                .unwrap_err()
                .contains("-> script ->"),
            "{kind}: no diagnostic may name the desugared script node"
        );
    }
}

#[test]
fn timeout_and_cancelation_validate_in_template_scope() {
    // `timeout`/`notifyPeriodInSeconds` are plain @fmtstring (job-creation
    // stage): Session.* is not in scope, exactly as for an authored Action.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "python": {
            "script": "print(1)",
            "timeout": "{{Param.T}}",
            "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{Param.N}}"}
        }}]
    }"#,
        &[
            "steps[0] -> python -> timeout:\n\tFailed to parse interpolation expression at [0, 11]. Undefined variable: 'Param.T'.",
            "steps[0] -> python -> cancelation:\n\tFailed to parse interpolation expression at [0, 11]. Undefined variable: 'Param.N'.",
        ],
    );
}

#[test]
fn session_symbol_in_timeout_is_rejected() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "node": {"script": "x", "timeout": "{{Session.WorkingDirectory}}"}}]
    }"#,
        &["steps[0] -> node -> timeout:\n\tFailed to parse interpolation expression at [0, 28]. Undefined variable: 'Session.WorkingDirectory'."],
    );
}

#[test]
fn let_without_expr_is_rejected_at_sugar_let() {
    // Previously the SimpleAction `let` was only examined when EXPR was on,
    // so a `let` without EXPR passed `check` and was silently ignored.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["x = 1"], "script": "echo"}}]
    }"#,
        &["steps[0] -> bash -> let:\n\t'let' requires the EXPR extension."],
    );
}

#[test]
fn let_binding_errors_carry_the_binding_index() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"let": ["x = 1", "y = 2", "x = 3"], "script": "echo"}}]
    }"#,
        &["steps[0] -> bash -> let[2]:\n\tduplicate name 'x'."],
    );
}

#[test]
fn comprehension_var_shadowing_let_reported_at_script_and_args() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {
            "let": ["x = 1"],
            "script": "echo {{ [x for x in [1, 2]] }}",
            "args": ["{{ [x for x in [1, 2]] }}"]
        }}]
    }"#,
        &[
            "steps[0] -> bash -> script:\n\tList comprehension variable 'x' shadows a let binding",
            "steps[0] -> bash -> args[0]:\n\tList comprehension variable 'x' shadows a let binding",
        ],
    );
}

#[test]
fn complex_expression_without_expr_reported_at_authored_arg() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{"name": "N", "type": "INT", "default": 1}],
        "steps": [{"name": "S", "bash": {"script": "echo", "args": ["{{Param.N + 1}}"]}}]
    }"#,
        &["steps[0] -> bash -> args[0]:\n\tcomplex expressions require the EXPR extension."],
    );
}

#[test]
fn well_formed_simple_actions_pass() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "parameterDefinitions": [{"name": "N", "type": "INT", "default": 1}],
        "steps": [
            {"name": "A", "bash": {"let": ["x = Param.N * 2"], "script": "echo {{x}} {{Session.WorkingDirectory}}", "args": ["{{Param.N}}"], "timeout": "{{Param.N}}"}},
            {"name": "B", "cmd": {"script": "echo %1", "args": ["{{Param.N}}"]}},
            {"name": "C", "powershell": {"script": "Write-Host $args", "args": ["-x", "{{Param.N}}"]}},
            {"name": "D", "python": {"script": "print({{Param.N}})", "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{Param.N}}"}}},
            {"name": "E", "node": {"script": "console.log({{Param.N}})"}}
        ]
    }"#,
    );
}

#[test]
fn sugar_may_reference_its_own_generated_file() {
    // The generated embedded file is `<sanitized step name>_script`; it is
    // in task scope for the sugar's `args`, `script`, and `let` exactly as
    // for the desugared form.
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "My Step", "bash": {
            "let": ["here = Task.File.My_Step_script"],
            "script": "echo {{here}} {{Task.File.My_Step_script.name}}",
            "args": ["{{Task.File.My_Step_script}}"]
        }}]
    }"#,
    );
}

#[test]
fn undefined_task_file_in_sugar_args_reported_at_authored_paths() {
    // Pass 6 reports the dangling reference at the script node, which for
    // sugar is the field itself; pass 8 reports the undefined symbol at
    // the authored arg. Neither names the desugared form.
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "python": {"script": "x", "args": ["{{Task.File.Other}}"]}}]
    }"#;
    check_err(
        s,
        &[
            "steps[0] -> python:\n\treferences undefined embedded file 'Other'.",
            "steps[0] -> python -> args[0]:\n\tFailed to parse interpolation expression at [0, 19]. Undefined variable: 'Task.File.Other'.",
        ],
    );
    let msg = decode_with(s, &CallerLimits::default()).unwrap_err();
    assert!(
        !msg.contains("-> script ->") && !msg.contains("embeddedFiles") && !msg.contains("onRun"),
        "no diagnostic may name the desugared form:\n{msg}"
    );
}

#[test]
fn args_index_for_single_prefix_kinds() {
    // bash/python/node: exactly one synthesized arg (the file reference).
    for kind in ["bash", "python", "node"] {
        let s = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "extensions": ["FEATURE_BUNDLE_1"],
            "name": "Test",
            "steps": [{{"name": "S", "{kind}": {{"script": "x", "args": ["a", "b", "{{{{Param.Nope}}}}"]}}}}]
        }}"#
        );
        check_err(
            &s,
            &[&format!(
                "steps[0] -> {kind} -> args[2]:\n\tFailed to parse interpolation expression at [0, 14]. Undefined variable: 'Param.Nope'."
            )],
        );
    }
}

#[test]
fn script_plus_sugar_reports_only_the_conflict() {
    // Pass 7 rejects the combination. The sugar is not additionally
    // validated: doing so against a step whose `Task.File.*` come from the
    // authored `script` would report the generated file reference as
    // undefined — a message naming a node the author never wrote.
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "script": {"actions": {"onRun": {"command": "echo"}}},
            "node": {"script": "x"}
        }]
    }"#;
    let msg = decode_with(s, &CallerLimits::default()).unwrap_err();
    assert_contains(
        &msg,
        &["steps[0] -> node:\n\tcannot have both 'node' and 'script'."],
    );
    assert!(
        !msg.contains("_script") && msg.starts_with("Model validation error: 1 validation error"),
        "only the conflict may be reported:\n{msg}"
    );
}

#[test]
fn two_sugar_fields_each_validated_at_their_own_path() {
    // Pass 7 rejects the pair; each field is still validated in full and
    // reported at its own path (they share the generated file name, so
    // the task symtab is right for both).
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{
            "name": "S",
            "bash": {"script": "{{Param.A}}"},
            "python": {"script": "{{Param.B}}"}
        }]
    }"#,
        &[
            "steps[0]:\n\tcannot have more than one simple action field.",
            "steps[0] -> bash -> script:\n\tFailed to parse interpolation expression at [0, 11]. Undefined variable: 'Param.A'.",
            "steps[0] -> python -> script:\n\tFailed to parse interpolation expression at [0, 11]. Undefined variable: 'Param.B'.",
        ],
    );
}

#[test]
fn non_ascii_step_name_desugars_and_validates() {
    // Used to panic in `desugar` at the generated file reference (the
    // sanitizer kept `char::is_alphanumeric` characters the expression
    // parser rejects). Now sanitized to ASCII as in the Python reference.
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [
            {"name": "²x", "bash": {"script": "echo"}},
            {"name": "a½", "python": {"script": "x"}},
            {"name": "١", "node": {"script": "x", "args": ["{{Task.File.__script}}"]}}
        ]
    }"#,
    );
}

// ══════════════════════════════════════════════════════════════
// Template validation — parse-time and structural (passes 0/6)
// ══════════════════════════════════════════════════════════════

#[test]
fn malformed_script_format_string_fails_at_parse() {
    // `script` is a `<DataString>` (@fmtstring[host]); before, this parsed
    // as a plain string, passed `check`, and only failed inside
    // `create_job` with no field path at all. Deserialization errors carry
    // no field path for authored format-string fields either, so only the
    // message is asserted here.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": "echo {{ "}}]
    }"#,
        &["Failed to parse interpolation expression at [5, 8]. Reason: Braces mismatch."],
    );
}

#[test]
fn empty_script_is_rejected() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": ""}}]
    }"#,
        &["steps[0] -> bash -> script:\n\tmust not be empty."],
    );
}

#[test]
fn empty_args_list_is_rejected() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": "echo", "args": []}}]
    }"#,
        &["steps[0] -> bash -> args:\n\tif provided, must not be empty."],
    );
}

#[test]
fn literal_timeout_and_notify_period_range_checks() {
    // The numeric literal checks an authored Action gets at
    // `steps[0] -> script -> actions -> onRun` land on the sugar field.
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {
            "script": "echo",
            "timeout": 0,
            "cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": 700}
        }}]
    }"#,
        &[
            "steps[0] -> bash:\n\tnotifyPeriodInSeconds must not exceed 600.",
            "steps[0] -> bash:\n\ttimeout must be > 0.",
        ],
    );
}

#[test]
fn control_characters_in_arg_reported_at_authored_index() {
    check_err(
        "{
        \"specificationVersion\": \"jobtemplate-2023-09\",
        \"extensions\": [\"FEATURE_BUNDLE_1\"],
        \"name\": \"Test\",
        \"steps\": [{\"name\": \"S\", \"powershell\": {\"script\": \"echo\", \"args\": [\"fine\", \"bad\\u0007\"]}}]
    }",
        &["steps[0] -> powershell -> args[1]:\n\tcontains control characters."],
    );
}

// ══════════════════════════════════════════════════════════════
// Template validation — opt-in resolved-value caps (gate 1)
// ══════════════════════════════════════════════════════════════

#[test]
fn static_script_over_data_cap_fails_check() {
    // Follow-up item 4's motivating case: a 500-char `bash:` body under
    // `max_resolved_data_len: 100` used to pass `check` and fail only at
    // `create_job`, at `steps[0] -> script -> embeddedFiles[0] -> data`.
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "bash": {{"script": "{}"}}}}]
    }}"#,
        "A".repeat(500)
    );
    check_err_with(
        &s,
        &data_cap(100),
        &["steps[0] -> bash -> script:\n\tis 500 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn partially_static_script_over_data_cap_fails_check_by_lower_bound() {
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "steps": [{"name": "S", "bash": {"script": "{{Session.WorkingDirectory}}/{{ 'A' * 200 }}"}}]
    }"#;
    check_err_with(
        s,
        &data_cap(100),
        &["steps[0] -> bash -> script:\n\tresolves to at least 201 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn static_arg_over_arg_cap_fails_check_at_authored_index() {
    let s = format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{{"name": "S", "cmd": {{"script": "echo", "args": ["short", "{}"]}}}}]
    }}"#,
        "B".repeat(300)
    );
    check_err_with(
        &s,
        &arg_cap(100),
        &["steps[0] -> cmd -> args[1]:\n\tis 300 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn synthesized_file_reference_is_not_measured_against_arg_cap() {
    // The generated `{{Task.File.<name>}}` arg is unresolved at every
    // client stage (contributes 0 to the bound), and the literal
    // interpreter args are short; a tiny cap must not trip on them.
    let s = r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "steps": [{"name": "S", "powershell": {"script": "echo"}}]
    }"#;
    decode_with(s, &arg_cap(10)).expect("only synthesized args; must pass");
}

// ══════════════════════════════════════════════════════════════
// Job creation (gate 2) — same checks, same authored paths
// ══════════════════════════════════════════════════════════════

fn param_script_template(kind: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1"],
        "name": "Test",
        "parameterDefinitions": [{{"name": "X", "type": "STRING"}}],
        "steps": [{{"name": "S", "{kind}": {{"script": "{{{{Param.X}}}}", "args": ["--flag", "{{{{Param.X}}}}"]}}}}]
    }}"#
    )
}

#[test]
fn script_over_data_cap_from_param_fails_at_create_job_at_sugar_path() {
    assert_contains(
        &create_with_limits(
            &param_script_template("bash"),
            &[("X", &"A".repeat(200))],
            data_cap(100),
        )
        .expect_err("over-cap data must fail at create_job"),
        &["steps[0] -> bash -> script:\n\tresolves to at least 200 characters, exceeding the maximum of 100."],
    );
}

#[test]
fn arg_over_arg_cap_from_param_fails_at_create_job_at_authored_index() {
    // Desugared cmd args: ["/C", <file>, "--flag", "{{Param.X}}"]; the
    // author's failing arg is args[1].
    let msg = create_with_limits(
        &param_script_template("cmd"),
        &[("X", &"A".repeat(200))],
        arg_cap(100),
    )
    .expect_err("over-cap arg must fail at create_job");
    assert_contains(
        &msg,
        &["steps[0] -> cmd -> args[1]:\n\tresolves to at least 200 characters, exceeding the maximum of 100."],
    );
    assert!(
        !msg.contains("-> script ->") && !msg.contains("embeddedFiles"),
        "no diagnostic may name the desugared form:\n{msg}"
    );
}

#[test]
fn simple_action_under_caps_creates_job() {
    let job = create_with_limits(
        &param_script_template("python"),
        &[("X", "short")],
        CallerLimits {
            max_resolved_data_len: Some(100),
            max_resolved_arg_len: Some(100),
            ..CallerLimits::default()
        },
    )
    .expect("under-cap SimpleAction must create a job");
    let action = &job.steps[0].script.actions.on_run;
    assert_eq!(action.command.raw(), "python");
    let args: Vec<&str> = action
        .args
        .as_ref()
        .unwrap()
        .iter()
        .map(|a| a.raw())
        .collect();
    assert_eq!(args, ["{{Task.File.S_script}}", "--flag", "{{Param.X}}"]);
}

// ── Job creation: template-scope action fields and `let`, re-rooted ──

fn param_timing_template(kind: &str, fields: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",
        "extensions": ["FEATURE_BUNDLE_1", "EXPR"],
        "name": "Test",
        "parameterDefinitions": [
            {{"name": "T", "type": "INT"}},
            {{"name": "M", "type": "STRING", "default": "TERMINATE"}}
        ],
        "steps": [{{"name": "S", "{kind}": {{"script": "echo", {fields}}}}}]
    }}"#
    )
}

#[test]
fn timeout_from_param_fails_at_create_job_at_sugar_timeout() {
    // Desugared path `script -> actions -> onRun -> timeout` re-roots
    // onto the authored `bash -> timeout`.
    let t = param_timing_template("bash", r#""timeout": "{{ Param.T }}""#);
    let msg = create_with_limits(&t, &[("T", "0")], CallerLimits::default())
        .expect_err("T = 0 must fail at create_job");
    assert_eq!(
        msg,
        "Model validation error: 1 validation error for JobTemplate\n\
         steps[0] -> bash -> timeout:\n\ttimeout must be > 0."
    );
    create_with_limits(&t, &[("T", "5")], CallerLimits::default())
        .expect("positive timeout must pass");
}

#[test]
fn notify_period_from_param_fails_at_create_job_at_sugar_cancelation() {
    let t = param_timing_template(
        "python",
        r#""cancelation": {"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": "{{ Param.T }}"}"#,
    );
    assert_contains(
        &create_with_limits(&t, &[("T", "601")], CallerLimits::default())
            .expect_err("T = 601 must fail at create_job"),
        &["steps[0] -> python -> cancelation:\n\tnotifyPeriodInSeconds must not exceed 600."],
    );
    create_with_limits(&t, &[("T", "60")], CallerLimits::default())
        .expect("in-range period must pass");
}

#[test]
fn deferred_mode_from_param_fails_at_create_job_at_sugar_cancelation() {
    let t = param_timing_template("cmd", r#""cancelation": {"mode": "{{ Param.M }}"}"#);
    assert_contains(
        &create_with_limits(&t, &[("T", "1"), ("M", "KILL")], CallerLimits::default())
            .expect_err("an invalid mode must fail at create_job"),
        &["steps[0] -> cmd -> cancelation:\n\tmode must resolve to TERMINATE or NOTIFY_THEN_TERMINATE, got 'KILL'."],
    );
}

#[test]
fn let_failure_at_create_job_reported_at_sugar_let_with_other_violations() {
    // The SimpleAction's `let` desugars to the script's `let`: a failure
    // at job creation re-roots onto `bash -> let[k]`, caret aligned to
    // the full binding, and is reported together with the action's other
    // violations (the timeout below).
    let t = param_timing_template(
        "bash",
        r#""let": ["ok = 1", "q = 1 / Param.T"], "timeout": "{{ Param.T }}""#,
    );
    let msg = create_with_limits(&t, &[("T", "0")], CallerLimits::default())
        .expect_err("T = 0 must fail at create_job");
    assert_eq!(
        msg,
        "Model validation error: 2 validation errors for JobTemplate\n\
         steps[0] -> bash -> let[1]:\n\tInvalid expression in let binding 'q': Division by zero\n  \
         q = 1 / Param.T\n      ~~^~~~~~~~~\n\
         steps[0] -> bash -> timeout:\n\ttimeout must be > 0."
    );
}
