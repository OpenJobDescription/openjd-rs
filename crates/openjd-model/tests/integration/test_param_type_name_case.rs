// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! §2: "When the `EXPR` extension is enabled, job parameter and task parameter type
//! names become case-insensitive." Without it they are case-sensitive.
//!
//! The conformance fixtures are
//! `base/job_templates/proposed/2--type-lowercase.invalid.yaml`
//! (openjd-specifications#163) for the base half and
//! `EXPR/job_templates/proposed/3.4.1--task-param-type-case-insensitive.yaml` (#166)
//! for the EXPR half. `EXPR/job_templates/2--type-case-insensitive.yaml` is already
//! gating and must stay passing.
//!
//! One check governs all three surfaces a parameter type name can appear on — a job
//! template's `parameterDefinitions`, an environment template's, and a step's
//! `taskParameterDefinitions` — so they are tested together here rather than split
//! across the three files that own those documents.
//!
//! Every surface is covered in all four combinations of EXPR on/off by canonical or
//! mis-cased spelling. Gold standard: failure tests assert the full error message
//! including the path.

use openjd_model::{decode_environment_template, decode_job_template, CallerLimits};

fn yaml_val(s: &str) -> serde_json::Value {
    serde_saphyr::from_str(s).unwrap()
}

/// Caller allowlists everything. What varies per test is whether the template
/// declares an extension, because the effective set is the intersection.
const ALLOWED: &[&str] = &["EXPR", "TASK_CHUNKING"];

fn decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_job_template(v, Some(ALLOWED), &CallerLimits::default())
        .unwrap_or_else(|e| panic!("Expected success for:\n{s}\nGot: {e}"));
}

fn check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_job_template(v, Some(ALLOWED), &CallerLimits::default())
        .expect_err(&format!("Expected error for:\n{s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

fn env_decode_ok(s: &str) {
    let v = yaml_val(s);
    decode_environment_template(v, Some(ALLOWED))
        .unwrap_or_else(|e| panic!("Expected success for:\n{s}\nGot: {e}"));
}

fn env_check_err(s: &str, expected: &[&str]) {
    let v = yaml_val(s);
    let err = decode_environment_template(v, Some(ALLOWED))
        .expect_err(&format!("Expected error for:\n{s}"));
    let msg = err.to_string();
    for line in expected {
        assert!(
            msg.contains(line),
            "Missing in error output: {line:?}\nGot:\n{msg}"
        );
    }
}

/// A job template with one job parameter definition. `extensions` is the raw YAML
/// line, empty for a template that declares none.
fn job_param(extensions: &str, type_name: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",{extensions}
        "name": "T",
        "parameterDefinitions": [{{"name": "P", "type": "{type_name}"}}],
        "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
    }}"#
    )
}

/// A job template with one task parameter definition.
fn task_param(extensions: &str, type_name: &str, range: &str, chunks: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "jobtemplate-2023-09",{extensions}
        "name": "T",
        "steps": [{{
            "name": "S",
            "parameterSpace": {{"taskParameterDefinitions": [
                {{"name": "F", "type": "{type_name}", "range": {range}{chunks}}}
            ]}},
            "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
        }}]
    }}"#
    )
}

/// An environment template with one parameter definition.
fn env_param(extensions: &str, type_name: &str) -> String {
    format!(
        r#"{{
        "specificationVersion": "environment-2023-09",{extensions}
        "parameterDefinitions": [{{"name": "P", "type": "{type_name}"}}],
        "environment": {{"name": "E", "script": {{"actions": {{"onEnter": {{"command": "foo"}}}}}}}}
    }}"#
    )
}

const NONE: &str = "";
const EXPR: &str = r#""extensions": ["EXPR"],"#;
const CHUNKING: &str = r#""extensions": ["TASK_CHUNKING"],"#;
const EXPR_CHUNKING: &str = r#""extensions": ["EXPR", "TASK_CHUNKING"],"#;
const CHUNKS: &str = r#", "chunks": {"defaultTaskCount": 1, "rangeConstraint": "CONTIGUOUS"}"#;

// ══════════════════════════════════════════════════════════════
// Job parameters — the four cases
// ══════════════════════════════════════════════════════════════

/// Case 1 of 4. Without EXPR the spec spelling is the only accepted one, and it is
/// accepted. Negative control: it proves a case-2 rejection is about the spelling
/// and not about the type being unavailable.
#[test]
fn test_job_no_expr_canonical_accepted() {
    for type_name in ["STRING", "INT", "FLOAT", "PATH"] {
        decode_ok(&job_param(NONE, type_name));
    }
}

/// Case 2 of 4. Without EXPR, type names are case-sensitive.
#[test]
fn test_job_no_expr_miscased_rejected() {
    check_err(
        &job_param(NONE, "string"),
        &["parameterDefinitions[0]:\n\tparameter type 'string' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'STRING'."],
    );
    check_err(
        &job_param(NONE, "StRiNg"),
        &["parameterDefinitions[0]:\n\tparameter type 'StRiNg' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'STRING'."],
    );
    check_err(
        &job_param(NONE, "int"),
        &["parameterDefinitions[0]:\n\tparameter type 'int' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'INT'."],
    );
}

/// Case 3 of 4. Enabling EXPR must not break the spec spelling.
#[test]
fn test_job_with_expr_canonical_accepted() {
    for type_name in ["STRING", "INT", "FLOAT", "PATH", "BOOL", "LIST[INT]"] {
        decode_ok(&job_param(EXPR, type_name));
    }
}

/// Case 4 of 4. With EXPR a mis-cased spelling is equivalent to the canonical one.
/// `EXPR/job_templates/2--type-case-insensitive.yaml` is the gating fixture here.
#[test]
fn test_job_with_expr_miscased_accepted() {
    for type_name in [
        "string",
        "sTrInG",
        "int",
        "Float",
        "pAtH",
        "bool",
        "range_expr",
        "list[int]",
        "List[String]",
        "list[list[int]]",
    ] {
        decode_ok(&job_param(EXPR, type_name));
    }
}

/// An EXPR-only type spelled canonically without EXPR is rejected for needing the
/// extension, which is a different rejection from the casing one. This is the
/// message eight tests in `test_expr_parameters.rs` assert, and the reason the
/// canonical-case check deliberately skips a name it recognizes as canonical.
#[test]
fn test_job_no_expr_canonical_expr_only_type_reports_not_allowed() {
    check_err(
        &job_param(NONE, "LIST[INT]"),
        &["parameterDefinitions[0]:\n\tparameter type 'LIST[INT]' is not allowed."],
    );
}

/// Mis-cased *and* EXPR-only without EXPR reports the casing, naming what the author
/// wrote. Before this change it reported `'LIST[INT]' is not allowed.`, laundering a
/// spelling the template never used.
#[test]
fn test_job_no_expr_miscased_expr_only_type_reports_the_casing() {
    check_err(
        &job_param(NONE, "list[int]"),
        &["parameterDefinitions[0]:\n\tparameter type 'list[int]' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'LIST[INT]'."],
    );
}

// ══════════════════════════════════════════════════════════════
// Task parameters — the four cases
// ══════════════════════════════════════════════════════════════

/// Case 1 of 4.
#[test]
fn test_task_no_expr_canonical_accepted() {
    decode_ok(&task_param(NONE, "INT", r#""1-3""#, ""));
    decode_ok(&task_param(NONE, "FLOAT", r#"["1.0", "2.0"]"#, ""));
    decode_ok(&task_param(NONE, "STRING", r#"["fg", "bg"]"#, ""));
    decode_ok(&task_param(NONE, "PATH", r#"["/tmp/a"]"#, ""));
    decode_ok(&task_param(CHUNKING, "CHUNK[INT]", r#""1-3""#, CHUNKS));
}

/// Case 2 of 4. This is the case that regresses if the fold is applied without
/// consulting the extension set, which is what making task parameter
/// deserialization case-blind does on its own.
#[test]
fn test_task_no_expr_miscased_rejected() {
    check_err(
        &task_param(NONE, "int", r#""1-3""#, ""),
        &["steps[0] -> parameterSpace -> taskParameterDefinitions[0]:\n\ttask parameter type 'int' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'INT'."],
    );
    check_err(
        &task_param(NONE, "Float", r#"["1.0", "2.0"]"#, ""),
        &["taskParameterDefinitions[0]:\n\ttask parameter type 'Float' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'FLOAT'."],
    );
    check_err(
        &task_param(NONE, "sTrInG", r#"["fg", "bg"]"#, ""),
        &["taskParameterDefinitions[0]:\n\ttask parameter type 'sTrInG' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'STRING'."],
    );
    check_err(
        &task_param(NONE, "pAtH", r#"["/tmp/a"]"#, ""),
        &["taskParameterDefinitions[0]:\n\ttask parameter type 'pAtH' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'PATH'."],
    );
}

/// Case 3 of 4.
#[test]
fn test_task_with_expr_canonical_accepted() {
    decode_ok(&task_param(EXPR, "INT", r#""1-3""#, ""));
    decode_ok(&task_param(EXPR, "PATH", r#"["/tmp/a"]"#, ""));
    decode_ok(&task_param(EXPR_CHUNKING, "CHUNK[INT]", r#""1-3""#, CHUNKS));
}

/// Case 4 of 4. These four spellings are exactly what
/// `3.4.1--task-param-type-case-insensitive.yaml` asserts.
#[test]
fn test_task_with_expr_miscased_accepted() {
    decode_ok(&task_param(EXPR, "int", r#""1-3""#, ""));
    decode_ok(&task_param(EXPR, "Float", r#"["1.0", "2.0"]"#, ""));
    decode_ok(&task_param(EXPR, "sTrInG", r#"["fg", "bg"]"#, ""));
    decode_ok(&task_param(EXPR, "pAtH", r#"["/tmp/a"]"#, ""));
}

/// `CHUNK[INT]` is a task parameter type name per §3.4.1.5, so EXPR makes
/// `chunk[int]` a spelling of it. No conformance fixture covers this.
#[test]
fn test_task_chunk_int_miscased_accepted_with_expr() {
    decode_ok(&task_param(EXPR_CHUNKING, "chunk[int]", r#""1-3""#, CHUNKS));
    decode_ok(&task_param(EXPR_CHUNKING, "Chunk[Int]", r#""1-3""#, CHUNKS));
}

/// TASK_CHUNKING supplies the type; EXPR is what makes its name case-insensitive.
/// Declaring only TASK_CHUNKING must still reject `chunk[int]`, which pins that the
/// check reads EXPR specifically rather than "any extension".
#[test]
fn test_task_chunk_int_miscased_needs_expr_not_only_task_chunking() {
    check_err(
        &task_param(CHUNKING, "chunk[int]", r#""1-3""#, CHUNKS),
        &["taskParameterDefinitions[0]:\n\ttask parameter type 'chunk[int]' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'CHUNK[INT]'."],
    );
}

// ══════════════════════════════════════════════════════════════
// Environment templates — the four cases
// ══════════════════════════════════════════════════════════════
//
// An environment template's `parameterDefinitions` is the same
// `JobParameterDefinition` union a job template's is, and it decodes through a
// different entry point. Auditing the equivalent Python change found this surface
// covered by nothing, so it is covered explicitly here.

/// Case 1 of 4.
#[test]
fn test_env_no_expr_canonical_accepted() {
    for type_name in ["STRING", "INT", "PATH"] {
        env_decode_ok(&env_param(NONE, type_name));
    }
}

/// Case 2 of 4. Fails if the check is wired into `decode_job_template` only.
#[test]
fn test_env_no_expr_miscased_rejected() {
    env_check_err(
        &env_param(NONE, "string"),
        &["parameterDefinitions[0]:\n\tparameter type 'string' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'STRING'."],
    );
    env_check_err(
        &env_param(NONE, "iNt"),
        &["parameterDefinitions[0]:\n\tparameter type 'iNt' is not recognized. Type names are case-sensitive without the EXPR extension; expected 'INT'."],
    );
}

/// Case 3 of 4.
#[test]
fn test_env_with_expr_canonical_accepted() {
    for type_name in ["STRING", "INT", "PATH"] {
        env_decode_ok(&env_param(EXPR, type_name));
    }
}

/// Case 4 of 4.
#[test]
fn test_env_with_expr_miscased_accepted() {
    for type_name in ["string", "iNt", "pAtH"] {
        env_decode_ok(&env_param(EXPR, type_name));
    }
}

// ══════════════════════════════════════════════════════════════
// The fold is ASCII, so a non-ASCII lookalike is not a spelling variant
// ══════════════════════════════════════════════════════════════
//
// `str::to_uppercase` is Unicode-aware and folds each of these wholly into the
// type-name alphabet: U+0131 LATIN SMALL LETTER DOTLESS I to 'I', U+017F LONG S to
// 'S', U+FB02 ligature fl to 'FL', U+FB06 ligature st to 'ST'. Under it, `ıNT` was
// accepted as INT with and without EXPR. `to_ascii_uppercase` leaves them alone.
//
// These are the only tests that fail if the fold is left Unicode-aware, and the
// with-EXPR ones are the load-bearing half: without EXPR the canonical-case check
// would reject a non-ASCII name anyway.

const LOOKALIKES: &[&str] = &["\u{131}NT", "\u{17f}TRING", "\u{fb02}OAT", "\u{fb06}RING"];

#[test]
fn test_job_non_ascii_lookalike_rejected_with_expr() {
    for type_name in LOOKALIKES {
        check_err(
            &job_param(EXPR, type_name),
            &[&format!("unknown parameter type: '{type_name}'")],
        );
    }
}

#[test]
fn test_job_non_ascii_lookalike_rejected_without_expr() {
    for type_name in LOOKALIKES {
        check_err(
            &job_param(NONE, type_name),
            &[&format!("unknown parameter type: '{type_name}'")],
        );
    }
}

#[test]
fn test_task_non_ascii_lookalike_rejected_with_expr() {
    for type_name in LOOKALIKES {
        check_err(
            &task_param(EXPR, type_name, r#""1-3""#, ""),
            &[&format!("unknown task parameter type: '{type_name}'")],
        );
    }
}

#[test]
fn test_task_non_ascii_lookalike_rejected_without_expr() {
    for type_name in LOOKALIKES {
        check_err(
            &task_param(NONE, type_name, r#""1-3""#, ""),
            &[&format!("unknown task parameter type: '{type_name}'")],
        );
    }
}

// ══════════════════════════════════════════════════════════════
// Boundaries of the check
// ══════════════════════════════════════════════════════════════

/// A spelling that names no type at all is left to deserialization, so it reports
/// once as an unknown type rather than twice.
#[test]
fn test_unknown_type_name_reports_once_and_not_as_a_casing_error() {
    check_err(
        &job_param(NONE, "NOPE"),
        &["unknown parameter type: 'NOPE'"],
    );
    check_err(
        &task_param(NONE, "NOPE", r#""1-3""#, ""),
        &["unknown task parameter type: 'NOPE'"],
    );
    // Not reported as a casing error, which would be the wrong diagnosis.
    let v = yaml_val(&job_param(NONE, "NOPE"));
    let msg = decode_job_template(v, Some(ALLOWED), &CallerLimits::default())
        .expect_err("expected error")
        .to_string();
    assert!(
        !msg.contains("is not recognized. Type names are case-sensitive"),
        "unknown type reported as a casing error:\n{msg}"
    );
}

/// A template declaring EXPR to a caller that does not allowlist it does not get
/// EXPR, so a mis-cased name stays rejected. Unlike the Python equivalent, the
/// unsupported-extension error and the casing error are collected together, so this
/// does pin the check rather than only the intersection.
#[test]
fn test_expr_declared_but_not_allowlisted_still_rejects_miscased() {
    let v = yaml_val(&job_param(EXPR, "string"));
    let msg = decode_job_template(v, None, &CallerLimits::default())
        .expect_err("expected error")
        .to_string();
    assert!(
        msg.contains("Unsupported extension names: EXPR"),
        "missing the extension error:\n{msg}"
    );
    assert!(
        msg.contains("parameter type 'string' is not recognized"),
        "missing the casing error:\n{msg}"
    );
}

/// An explicit `null` on either optional field means absent, and stays accepted.
/// The walk must not read through it, so this is the case that would panic on an
/// unguarded `as_array().unwrap()`.
#[test]
fn test_explicit_null_parameter_fields_stay_accepted() {
    decode_ok(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "parameterDefinitions": null,
        "steps": [{"name": "S", "parameterSpace": null,
                   "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
    );
}

/// The walk skips anything that is not an object with a string `type`, leaving
/// deserialization to report it. These must be validation errors, not panics.
#[test]
fn test_malformed_parameter_definitions_are_errors_not_panics() {
    for defs in [
        "{}",
        r#""int""#,
        r#"["int"]"#,
        r#"[{"name": "P", "type": 3}]"#,
        r#"[{"name": "P", "type": null}]"#,
        r#"[{"name": "P"}]"#,
    ] {
        let s = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "T",
            "parameterDefinitions": {defs},
            "steps": [{{"name": "S", "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}}}]
        }}"#
        );
        let v = yaml_val(&s);
        assert!(
            decode_job_template(v, Some(ALLOWED), &CallerLimits::default()).is_err(),
            "expected an error for parameterDefinitions: {defs}"
        );
    }
}

/// Same for the task-parameter shape, which the walk reaches through
/// `steps[*].parameterSpace`.
#[test]
fn test_malformed_task_parameter_definitions_are_errors_not_panics() {
    for space in [
        "{}",
        r#"{"taskParameterDefinitions": null}"#,
        r#"{"taskParameterDefinitions": "int"}"#,
        r#"{"taskParameterDefinitions": ["int"]}"#,
        r#"{"taskParameterDefinitions": [{"name": "F", "type": 3, "range": "1-3"}]}"#,
        r#"{"taskParameterDefinitions": [{"name": "F", "range": "1-3"}]}"#,
    ] {
        let s = format!(
            r#"{{
            "specificationVersion": "jobtemplate-2023-09",
            "name": "T",
            "steps": [{{
                "name": "S",
                "parameterSpace": {space},
                "script": {{"actions": {{"onRun": {{"command": "foo"}}}}}}
            }}]
        }}"#
        );
        let v = yaml_val(&s);
        assert!(
            decode_job_template(v, Some(ALLOWED), &CallerLimits::default()).is_err(),
            "expected an error for parameterSpace: {space}"
        );
    }
}

/// A mis-cased name on a second step is reported at that step's path, so the walk
/// indexes steps rather than reporting everything at `steps[0]`.
#[test]
fn test_miscased_task_type_on_a_later_step_reports_that_steps_path() {
    check_err(
        r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "steps": [
            {"name": "A", "script": {"actions": {"onRun": {"command": "foo"}}}},
            {"name": "B",
             "parameterSpace": {"taskParameterDefinitions": [{"name": "F", "type": "int", "range": "1-3"}]},
             "script": {"actions": {"onRun": {"command": "foo"}}}}
        ]
    }"#,
        &["steps[1] -> parameterSpace -> taskParameterDefinitions[0]:\n\ttask parameter type 'int' is not recognized."],
    );
}

/// Every mis-cased name is reported, not just the first, matching the collect-all
/// behaviour of the rest of the parse phase.
#[test]
fn test_all_miscased_names_are_reported() {
    let msg = decode_job_template(
        yaml_val(
            r#"{
        "specificationVersion": "jobtemplate-2023-09",
        "name": "T",
        "parameterDefinitions": [{"name": "P", "type": "string"}, {"name": "Q", "type": "int"}],
        "steps": [{"name": "S",
            "parameterSpace": {"taskParameterDefinitions": [{"name": "F", "type": "path", "range": ["/tmp/a"]}]},
            "script": {"actions": {"onRun": {"command": "foo"}}}}]
    }"#,
        ),
        Some(ALLOWED),
        &CallerLimits::default(),
    )
    .expect_err("expected error")
    .to_string();
    for needle in [
        "parameterDefinitions[0]",
        "parameterDefinitions[1]",
        "taskParameterDefinitions[0]",
        "expected 'STRING'",
        "expected 'INT'",
        "expected 'PATH'",
    ] {
        assert!(msg.contains(needle), "missing {needle:?} in:\n{msg}");
    }
}
